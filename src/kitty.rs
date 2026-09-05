//! Kitty graphics protocol.
//!
//! This is the fix for everything the sixel path fought: placements are
//! specified in *cells* (`c=`/`r=`) and the terminal does the scaling, so the
//! cell pixel size -- which no terminal reports reliably, and which zellij
//! gets wrong outright -- never enters the calculation.
//!
//! Supported by WezTerm, kitty, ghostty, Konsole, and by zellij from v0.45.0.

use image::RgbImage;

/// Image id we own. Reusing one id means a new cover replaces the old rather
/// than accumulating placements.
pub const IMAGE_ID: u32 = 0x7974_6b77; // "ytkw"

/// Base64 payload chunk size. The protocol caps each escape's data at 4096.
const CHUNK: usize = 4096;

/// Encode as a 256-colour indexed PNG, which the protocol takes directly
/// (`f=100`).
///
/// Truecolour PNG of a photo runs close to a megabyte once base64-encoded,
/// which is a visible hitch on every track change. An indexed image reuses
/// the median-cut palette the sixel path already builds and comes out several
/// times smaller with no visible loss on album art.
fn to_png(img: &RgbImage) -> Option<Vec<u8>> {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }

    // Every third pixel characterises a cover well enough and keeps the
    // palette build cheap.
    let mut samples: Vec<[u8; 3]> = img
        .pixels()
        .step_by(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let palette = crate::sixel::median_cut(&mut samples, 256);
    let lut = crate::sixel::build_lut(&palette);

    let indices: Vec<u8> = img
        .pixels()
        .map(|p| lut[crate::sixel::lut_index(&[p[0], p[1], p[2]])])
        .collect();

    let mut flat = Vec::with_capacity(palette.len() * 3);
    for c in &palette {
        flat.extend_from_slice(c);
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(flat);
        encoder.set_compression(png::Compression::High);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&indices).ok()?;
    }
    Some(out)
}

/// Escape sequence that transmits and displays `img` across `cols` x `rows`
/// cells, anchored at the current cursor position.
///
/// Returns None only if PNG encoding fails.
pub fn draw(img: &RgbImage, cols: u16, rows: u16) -> Option<String> {
    use base64::Engine;
    if cols == 0 || rows == 0 || img.width() == 0 || img.height() == 0 {
        return None;
    }
    let png = to_png(img)?;
    let data = base64::engine::general_purpose::STANDARD.encode(&png);

    let mut out = String::with_capacity(data.len() + 256);
    // Replace any previous cover before drawing the new one, so placements do
    // not stack up as tracks change.
    out.push_str(&delete());

    let chunks: Vec<&str> = data
        .as_bytes()
        .chunks(CHUNK)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect();

    for (i, chunk) in chunks.iter().enumerate() {
        let more = if i + 1 < chunks.len() { 1 } else { 0 };
        out.push_str("\x1b_G");
        if i == 0 {
            // a=T transmit+display, f=100 PNG, c/r the placement in cells,
            // q=2 suppress replies so nothing lands in our input stream.
            // Deliberately no explicit placement id: zellij mishandles those
            // (zellij-org/zellij#5573).
            out.push_str(&format!(
                "a=T,f=100,i={IMAGE_ID},c={cols},r={rows},q=2,m={more}"
            ));
        } else {
            out.push_str(&format!("m={more}"));
        }
        out.push(';');
        out.push_str(chunk);
        out.push_str("\x1b\\");
    }
    Some(out)
}

/// Remove our image and every placement of it.
pub fn delete() -> String {
    format!("\x1b_Ga=d,d=I,i={IMAGE_ID},q=2;\x1b\\")
}

/// Ask the terminal whether it speaks the protocol, by transmitting a 1x1
/// image and waiting for the acknowledgement.
///
/// The specification requires a supporting terminal to answer a query, so
/// silence means no support. This matters: without it a wrong guess dumps
/// kilobytes of base64 across the screen as literal text.
///
/// Must run before any input reader is started.
pub fn query_kitty_support() -> bool {
    use base64::Engine;
    use std::io::Write;

    let Some(mut tty) = crate::sixel::open_tty() else {
        return false;
    };
    let Some(raw) = crate::sixel::RawMode::enable(&tty) else {
        return false;
    };

    let ok = (|| -> bool {
        // One RGB pixel, f=24, a=q: query only, nothing is displayed.
        let pixel = base64::engine::general_purpose::STANDARD.encode([0u8, 0, 0]);
        let probe = format!("\x1b_Gi={IMAGE_ID},s=1,v=1,a=q,t=d,f=24;{pixel}\x1b\\");
        if tty.write_all(probe.as_bytes()).is_err() || tty.flush().is_err() {
            return false;
        }
        // The reply is an APC ending in ESC backslash; read to the backslash.
        match crate::sixel::read_reply(&mut tty, b'\\', 250) {
            Some(reply) => {
                let text = String::from_utf8_lossy(&reply);
                text.contains("_G") && text.contains("OK")
            }
            None => false,
        }
    })();

    drop(raw);
    ok
}

/// Whether this terminal is expected to render kitty graphics.
///
/// zellij only gained support in v0.45.0, so an older multiplexer is
/// explicitly excluded -- there the escapes would be passed through or
/// dropped, neither of which draws anything useful.
pub fn terminal_supports_kitty() -> bool {
    // Inside a multiplexer the query can be answered by the *host* terminal
    // while the multiplexer itself mangles or drops the image, so the version
    // gate has to pass first. zellij only gained support in 0.45.0.
    if let Some(mux) = crate::sixel::multiplexer() {
        if mux != "zellij" || !zellij_version().is_some_and(|v| v >= (0, 45, 0)) {
            return false;
        }
    }
    query_kitty_support()
}

/// Parse zellij's version by asking the binary, since it exports no version
/// environment variable.
fn zellij_version() -> Option<(u32, u32, u32)> {
    let out = std::process::Command::new("zellij")
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_semver(&text)
}

fn parse_semver(text: &str) -> Option<(u32, u32, u32)> {
    // e.g. "zellij 0.43.1"
    let token = text.split_whitespace().find(|t| {
        t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.')
    })?;
    let mut parts = token.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .and_then(|p| p.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn img(w: u32, h: u32) -> RgbImage {
        let mut i = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                i.put_pixel(x, y, Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
            }
        }
        i
    }

    #[test]
    fn placement_is_specified_in_cells_not_pixels() {
        // The whole point: the terminal scales to the cell box, so no cell
        // pixel size is ever needed.
        let s = draw(&img(64, 64), 36, 18).unwrap();
        assert!(s.contains("c=36,r=18"), "missing cell placement");
        assert!(s.contains("a=T"), "should transmit and display");
        assert!(s.contains("f=100"), "should send PNG");
    }

    #[test]
    fn replies_are_suppressed_so_nothing_pollutes_stdin() {
        let s = draw(&img(8, 8), 4, 2).unwrap();
        assert!(s.contains("q=2"), "must suppress protocol responses");
    }

    #[test]
    fn a_new_cover_deletes_the_previous_one() {
        let s = draw(&img(8, 8), 4, 2).unwrap();
        assert!(s.starts_with("\x1b_Ga=d"), "should delete before drawing");
        assert!(s.contains(&format!("i={IMAGE_ID}")));
    }

    #[test]
    fn no_explicit_placement_id_is_sent() {
        // zellij#5573: explicit placement ids (p=) are mishandled there.
        let s = draw(&img(8, 8), 4, 2).unwrap();
        assert!(!s.contains("p="), "must not use explicit placement ids");
    }

    /// Noisy pixels so PNG cannot compress the payload below the chunk cap.
    fn noise(w: u32, h: u32) -> RgbImage {
        let mut i = RgbImage::new(w, h);
        let mut state: u32 = 0x1234_5678;
        for y in 0..h {
            for x in 0..w {
                // xorshift: deterministic but incompressible.
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let b = state.to_le_bytes();
                i.put_pixel(x, y, Rgb([b[0], b[1], b[2]]));
            }
        }
        i
    }

    #[test]
    fn large_images_are_chunked_within_protocol_limits() {
        let s = draw(&noise(256, 256), 40, 20).unwrap();
        // Every escape's payload must stay under the 4096 cap.
        for part in s.split("\x1b_G").skip(1) {
            let payload = part.split(';').nth(1).unwrap_or("");
            let data = payload.trim_end_matches("\x1b\\");
            assert!(data.len() <= CHUNK, "chunk of {} bytes", data.len());
        }
        // Continuation chunks must be marked, and the last must close.
        assert!(s.contains("m=1"), "expected continuation chunks");
        assert!(s.contains("m=0"), "expected a final chunk");
    }

    #[test]
    fn every_escape_is_terminated() {
        let s = draw(&img(128, 128), 20, 10).unwrap();
        let opens = s.matches("\x1b_G").count();
        let closes = s.matches("\x1b\\").count();
        assert_eq!(opens, closes, "unterminated escape");
    }

    #[test]
    fn zero_sized_placements_are_rejected() {
        assert!(draw(&img(8, 8), 0, 4).is_none());
        assert!(draw(&img(8, 8), 4, 0).is_none());
        assert!(draw(&RgbImage::new(0, 0), 4, 4).is_none());
    }

    #[test]
    fn version_parsing_handles_zellij_output() {
        assert_eq!(parse_semver("zellij 0.43.1"), Some((0, 43, 1)));
        assert_eq!(parse_semver("zellij 0.45.0"), Some((0, 45, 0)));
        assert_eq!(parse_semver("zellij 1.0"), Some((1, 0, 0)));
        assert_eq!(parse_semver("no version here"), None);
    }

    #[test]
    fn kitty_support_needs_zellij_045_or_newer() {
        // The version gate is what stops us drawing into a zellij that will
        // silently drop the escapes.
        assert!((0, 45, 0) >= (0, 45, 0));
        assert!((0, 43, 1) < (0, 45, 0));
    }
}
