//! Sixel encoder for album art.
//!
//! Why hand-rolled rather than shelling out to chafa: chafa sizes its sixel
//! output from the terminal's cell pixel size, which it can only learn by
//! querying a TTY. Piping its output into a TUI means it never sees one, so it
//! falls back to a guess and the cover comes out vertically squashed, with no
//! flag to correct it. Encoding here gives exact pixel dimensions, drops a
//! runtime dependency, and reuses the image we already decoded.
//!
//! Quantisation is the 6x6x6 colour cube (216 entries) with a Bayer 4x4
//! ordered dither. That is cheap, deterministic across runs, and holds up well
//! on album art, where the alternative -- k-means over ~100k pixels for every
//! track change -- would cost far more than it returns.

use image::RgbImage;
use std::fmt::Write as _;

/// Palette size. libsixel defaults to 256; 128 adaptive entries look at
/// least as good as a 216-entry fixed cube while emitting far less data,
/// because each six-row band then touches fewer distinct colours and the
/// run-length encoding has longer runs to work with.
const PALETTE_SIZE: usize = 128;

/// Side of the RGB lookup cube used to map pixels onto the palette. 32 gives
/// 32k entries, cheap to build once and O(1) per pixel afterwards.
const LUT_BITS: u32 = 5;
const LUT_SIDE: usize = 1 << LUT_BITS;

/// Build an adaptive palette by median cut.
///
/// Repeatedly split the bucket with the widest channel spread at its median,
/// which concentrates palette entries where the image actually has detail --
/// the whole reason this beats a fixed cube at the same entry count.
pub(crate) fn median_cut(samples: &mut Vec<[u8; 3]>, n: usize) -> Vec<[u8; 3]> {
    if samples.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut buckets: Vec<Vec<[u8; 3]>> = vec![std::mem::take(samples)];

    while buckets.len() < n {
        // Find the bucket worth splitting: widest single-channel range.
        let mut best = None;
        let mut best_range = 0u16;
        let mut best_channel = 0usize;
        for (i, b) in buckets.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..3 {
                let (mut lo, mut hi) = (255u8, 0u8);
                for p in b.iter() {
                    lo = lo.min(p[ch]);
                    hi = hi.max(p[ch]);
                }
                let range = hi as u16 - lo as u16;
                if range > best_range {
                    best_range = range;
                    best = Some(i);
                    best_channel = ch;
                }
            }
        }
        let Some(i) = best else { break };
        if best_range == 0 {
            break;
        }

        let mut bucket = buckets.swap_remove(i);
        bucket.sort_unstable_by_key(|p| p[best_channel]);
        let mid = bucket.len() / 2;
        let upper = bucket.split_off(mid);
        buckets.push(bucket);
        buckets.push(upper);
    }

    buckets
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let n = b.len() as u32;
            let sum = b.iter().fold([0u32; 3], |mut acc, p| {
                acc[0] += p[0] as u32;
                acc[1] += p[1] as u32;
                acc[2] += p[2] as u32;
                acc
            });
            [
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
            ]
        })
        .collect()
}

/// Nearest-palette-entry lookup table over a coarse RGB cube.
pub(crate) fn build_lut(palette: &[[u8; 3]]) -> Vec<u8> {
    let shift = 8 - LUT_BITS;
    let mut lut = vec![0u8; LUT_SIDE * LUT_SIDE * LUT_SIDE];
    for r in 0..LUT_SIDE {
        for g in 0..LUT_SIDE {
            for b in 0..LUT_SIDE {
                // Centre of this LUT cell, in 0..255.
                let target = [
                    ((r as u32) << shift | (1 << shift) >> 1) as i32,
                    ((g as u32) << shift | (1 << shift) >> 1) as i32,
                    ((b as u32) << shift | (1 << shift) >> 1) as i32,
                ];
                let mut best = 0u8;
                let mut best_d = i32::MAX;
                for (i, p) in palette.iter().enumerate() {
                    let d = (p[0] as i32 - target[0]).pow(2)
                        + (p[1] as i32 - target[1]).pow(2)
                        + (p[2] as i32 - target[2]).pow(2);
                    if d < best_d {
                        best_d = d;
                        best = i as u8;
                    }
                }
                lut[(r << (LUT_BITS * 2)) | (g << LUT_BITS) | b] = best;
            }
        }
    }
    lut
}

#[inline]
pub(crate) fn lut_index(p: &[u8; 3]) -> usize {
    let shift = 8 - LUT_BITS;
    ((p[0] as usize >> shift) << (LUT_BITS * 2))
        | ((p[1] as usize >> shift) << LUT_BITS)
        | (p[2] as usize >> shift)
}

/// Encode an RGB image as a sixel payload, `ESC P` through `ESC \`.
///
/// The image is used at its own dimensions; resize before calling so the
/// result matches the cell area it must fill.
pub fn encode(img: &RgbImage) -> String {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return String::new();
    }

    // Build the palette from a subsample -- every 3rd pixel is plenty to
    // characterise an album cover and keeps the sort cost down.
    let mut samples: Vec<[u8; 3]> = img
        .pixels()
        .step_by(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let palette = median_cut(&mut samples, PALETTE_SIZE);
    let lut = build_lut(&palette);

    // Map every pixel to a palette index.
    let mut idx = vec![0u8; (w * h) as usize];
    let mut used = vec![false; palette.len()];
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            let i = lut[lut_index(&[p[0], p[1], p[2]])];
            idx[(y * w + x) as usize] = i;
            used[i as usize] = true;
        }
    }

    let mut out = String::with_capacity((w * h / 4) as usize + 1024);
    // P1=0 aspect, P2=0 (unset pixels keep background), P3=0.
    out.push_str("\x1bP0;0;0q");
    // Raster attributes: pan;pad;width;height.
    let _ = write!(out, "\"1;1;{w};{h}");

    // Colour registers, in percent as the format requires.
    for (i, is_used) in used.iter().enumerate() {
        if !is_used {
            continue;
        }
        let c = palette[i];
        let pct = |v: u8| (v as u32 * 100 + 127) / 255;
        let _ = write!(out, "#{};2;{};{};{}", i, pct(c[0]), pct(c[1]), pct(c[2]));
    }

    // Sixel data, six pixel rows per band.
    let bands = h.div_ceil(6);
    let mut bits_row: Vec<u8> = Vec::with_capacity(w as usize);
    for band in 0..bands {
        let y0 = band * 6;
        let rows = (h - y0).min(6);

        // Which colours appear in this band at all.
        let mut band_colors = Vec::new();
        let mut seen = vec![false; palette.len()];
        for dy in 0..rows {
            for x in 0..w {
                let c = idx[((y0 + dy) * w + x) as usize] as usize;
                if !seen[c] {
                    seen[c] = true;
                    band_colors.push(c);
                }
            }
        }
        band_colors.sort_unstable();

        let mut wrote_pass = false;
        for c in band_colors {
            let _ = &c;

            // Build this colour's bitmask row, then run-length encode it.
            // Compute the row first so trailing empty space can be dropped:
            // sixel does not require padding a pass out to the full width,
            // and for a 216-entry palette most passes are mostly empty.
            bits_row.clear();
            let mut last_set = None;
            for x in 0..w {
                let mut bits = 0u8;
                for dy in 0..rows {
                    if idx[((y0 + dy) * w + x) as usize] as usize == c {
                        bits |= 1 << dy;
                    }
                }
                if bits != 0 {
                    last_set = Some(x);
                }
                bits_row.push(bits);
            }
            let Some(last_set) = last_set else {
                continue;
            };
            if wrote_pass {
                // Graphics carriage return: overlay the next colour pass.
                out.push('$');
            }
            wrote_pass = true;
            let _ = write!(out, "#{c}");

            let mut run_char = 0u8;
            let mut run_len = 0u32;
            for &bits in &bits_row[..=last_set as usize] {
                let ch = 0x3F + bits;
                if run_len > 0 && ch == run_char {
                    run_len += 1;
                } else {
                    emit_run(&mut out, run_char, run_len);
                    run_char = ch;
                    run_len = 1;
                }
            }
            emit_run(&mut out, run_char, run_len);
        }
        // Graphics newline, except after the final band.
        if band + 1 < bands {
            out.push('-');
        }
    }

    out.push_str("\x1b\\");
    out
}

fn emit_run(out: &mut String, ch: u8, len: u32) {
    if len == 0 {
        return;
    }
    let c = ch as char;
    // The `!` repeat form only pays for itself past three characters.
    if len > 3 {
        let _ = write!(out, "!{len}{c}");
    } else {
        for _ in 0..len {
            out.push(c);
        }
    }
}


/// Where a cell size came from. Sixel needs the real cell size to scale an
/// image correctly, and a wrong value overflows the reserved area -- which
/// inside a multiplexer means painting over other panes. So the source is
/// tracked, and `auto` mode only enables sixel when it is trustworthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    /// Set explicitly in the config.
    Config,
    /// Measured by drawing a probe image and reading the cursor back. The
    /// only source that reflects what the terminal actually does with sixel.
    Calibrated,
    /// The terminal answered a CSI 16 t query. Good for the *aspect ratio*,
    /// but not for absolute scale: WezTerm on HiDPI answers in device pixels
    /// while laying sixel out in logical pixels, so the value can be a whole
    /// scale factor too large. Never trusted on its own.
    Query,
    /// Derived from the tty window size. Unreliable under multiplexers, which
    /// may report the outer window's pixels alongside the pane's cell count.
    Ioctl,
    /// Nothing usable; a conventional guess.
    Fallback,
}

impl CellSource {
    /// Only a definitive answer justifies drawing pixel graphics unasked.
    ///
    /// A `CSI 16 t` report is not enough on its own: it can be a whole scale
    /// factor off, and being too large means the cover paints over the track
    /// title. Only a pinned value or a measurement qualifies.
    pub fn is_trustworthy(self) -> bool {
        matches!(self, CellSource::Config | CellSource::Calibrated)
    }
}

const FALLBACK_CELL: (u16, u16) = (8, 16);

/// A cell narrower than 4px or taller than 64px is not a real font metric,
/// and neither is a wildly non-vertical aspect ratio.
fn plausible(cw: u16, ch: u16) -> bool {
    if !(4..=32).contains(&cw) || !(8..=64).contains(&ch) {
        return false;
    }
    let ratio = ch as f32 / cw as f32;
    (1.2..=3.5).contains(&ratio)
}

/// Resolve the cell size, preferring the most reliable source available.
pub fn detect_cell_size(config: Option<(u16, u16)>) -> ((u16, u16), CellSource) {
    if let Some((w, h)) = config {
        if w > 0 && h > 0 {
            return ((w, h), CellSource::Config);
        }
    }
    if let Some((w, h)) = query_cell_size() {
        if plausible(w, h) {
            return ((w, h), CellSource::Query);
        }
    }
    if let Some((w, h)) = ioctl_cell_size() {
        if plausible(w, h) {
            return ((w, h), CellSource::Ioctl);
        }
    }
    (FALLBACK_CELL, CellSource::Fallback)
}

/// Cell size from the tty window size, when the terminal fills it in.
fn ioctl_cell_size() -> Option<(u16, u16)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) != 0 {
            return None;
        }
        if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 || ws.ws_col == 0 || ws.ws_row == 0 {
            return None;
        }
        Some((ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row))
    }
}

/// Measure the cell size the terminal actually uses for *graphics*.
///
/// Reported cell sizes cannot be trusted for sixel: WezTerm on a HiDPI
/// display answers `CSI 16 t` in device pixels but lays sixel out in logical
/// pixels, so the reported value is the scale factor too large and the image
/// overflows by exactly that factor.
///
/// So measure it instead. Sixel advances the cursor by the number of rows the
/// image covered, so drawing a strip of known pixel height and reading the
/// cursor back gives the true rows-per-pixel. The width is then derived from
/// the reported aspect ratio, which is scale-invariant because both reported
/// numbers carry the same factor.
///
/// Must run on a quiet terminal, before any input reader is started.
pub fn calibrate(reported: (u16, u16)) -> Option<(u16, u16)> {
    // A taller probe measures more precisely, because the row count is
    // quantised: at 120px and a 25px cell the answer is only known to within
    // 20%, which is enough to overflow the layout. Bound it by the smallest
    // plausible cell so it can never scroll the screen, since scrolling
    // silently corrupts the reading.
    let rows_available = crossterm::terminal::size().map(|(_, r)| r).unwrap_or(24);
    let probe_px: u32 = (rows_available.saturating_sub(2) as u32 * 8)
        .min(reported.1 as u32 * 8)
        .clamp(96, 480);

    let mut tty = open_tty()?;
    let raw = RawMode::enable(&tty)?;

    let result = (|| -> Option<(u16, u16)> {
        use std::io::Write;
        // Park at the top-left before drawing. Measuring from the bottom of a
        // full screen makes the image scroll, and the cursor row then
        // saturates at the last line -- under-counting rows and so
        // over-estimating the cell height by whatever the overflow was.
        tty.write_all(b"\x1b7\x1b[1;1H").ok()?;
        tty.flush().ok()?;

        let strip = image::RgbImage::new(2, probe_px);
        let payload = encode(&strip);
        tty.write_all(payload.as_bytes()).ok()?;
        tty.flush().ok()?;
        let after = cursor_row(&mut tty)?;

        // Erase whatever the probe drew, then put the cursor back.
        tty.write_all(b"\x1b[1;1H\x1b[J\x1b8").ok()?;
        tty.flush().ok()?;

        // Started at row 1, so the advance is `after - 1`.
        let rows = after.saturating_sub(1);
        if rows == 0 {
            // Nothing was drawn, so sixel is not really supported here.
            return None;
        }
        // Sixel leaves the cursor on the row *containing* the bottom of the
        // image, so the advance is floor(height / cell), which means the true
        // cell height lies in (probe/(rows+1), probe/rows]. Take the lower
        // bound: under-estimating leaves a small gap below the cover, while
        // over-estimating paints the image over the track title.
        let cell_h = (probe_px as f32 / (rows + 1) as f32).floor().max(1.0) as u16;
        // Preserve the reported aspect; only the absolute scale was suspect.
        let ratio = reported.0 as f32 / reported.1.max(1) as f32;
        let cell_w = (cell_h as f32 * ratio).round().max(1.0) as u16;
        if plausible(cell_w, cell_h) {
            Some((cell_w, cell_h))
        } else {
            None
        }
    })();

    drop(raw);
    result
}

/// Ask the terminal for the cursor row via `CSI 6 n`, which answers
/// `CSI <row> ; <col> R`.
fn cursor_row(tty: &mut std::fs::File) -> Option<u16> {
    use std::io::Write;
    tty.write_all(b"\x1b[6n").ok()?;
    tty.flush().ok()?;
    let reply = read_reply(tty, b'R', 300)?;
    parse_cursor_row(&reply)
}

fn parse_cursor_row(bytes: &[u8]) -> Option<u16> {
    let s = std::str::from_utf8(bytes).ok()?;
    let start = s.rfind("\u{1b}[")?;
    let body = &s[start + 2..];
    let end = body.find('R')?;
    body[..end].split(';').next()?.trim().parse().ok()
}

/// Read from the tty until `terminator` arrives or the deadline passes.
pub(crate) fn read_reply(tty: &mut std::fs::File, terminator: u8, millis: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    let fd = tty.as_raw_fd();
    let mut buf = [0u8; 64];
    let mut got = 0usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
    while std::time::Instant::now() < deadline && got < buf.len() {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as i32;
        if unsafe { libc::poll(&mut poll, 1, remaining.max(1)) } <= 0 {
            break;
        }
        match tty.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => break,
        }
        if buf[..got].contains(&terminator) {
            break;
        }
    }
    if got == 0 {
        None
    } else {
        Some(buf[..got].to_vec())
    }
}

pub(crate) fn open_tty() -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()
}

/// Puts the tty in raw mode and restores the previous settings on drop.
pub(crate) struct RawMode {
    fd: i32,
    saved: libc::termios,
}

impl RawMode {
    pub(crate) fn enable(tty: &std::fs::File) -> Option<Self> {
        use std::os::unix::io::AsRawFd;
        let fd = tty.as_raw_fd();
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return None;
        }
        let mut raw = saved;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        Some(Self { fd, saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
    }
}

/// Ask the terminal for its cell size with `CSI 16 t`, which answers
/// `CSI 6 ; height ; width t`.
///
/// This is the only way to get a correct answer through a multiplexer, and it
/// must run before the TUI takes over the terminal.
fn query_cell_size() -> Option<(u16, u16)> {

    let mut tty = open_tty()?;
    let raw = RawMode::enable(&tty)?;
    let result = (|| -> Option<(u16, u16)> {
        use std::io::Write;
        tty.write_all(b"\x1b[16t").ok()?;
        tty.flush().ok()?;
        // Terminals that do not implement this never answer, so the read must
        // time out rather than hang startup.
        let reply = read_reply(&mut tty, b't', 200)?;
        parse_cell_report(&reply)
    })();
    drop(raw);
    result
}

/// Parse `CSI 6 ; <height> ; <width> t`.
fn parse_cell_report(bytes: &[u8]) -> Option<(u16, u16)> {
    let s = std::str::from_utf8(bytes).ok()?;
    // Find the report even if other input arrived alongside it.
    let start = s.find("\x1b[6;").or_else(|| s.find("\u{1b}[6;"))?;
    let rest = &s[start..];
    let body = rest.strip_prefix("\u{1b}[6;")?;
    let end = body.find('t')?;
    let mut parts = body[..end].split(';');
    let height: u16 = parts.next()?.trim().parse().ok()?;
    let width: u16 = parts.next()?.trim().parse().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Name of the terminal multiplexer in use, if any.
pub fn multiplexer() -> Option<&'static str> {
    if std::env::var_os("ZELLIJ").is_some() {
        return Some("zellij");
    }
    if std::env::var_os("TMUX").is_some() {
        return Some("tmux");
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term.starts_with("screen") || term.starts_with("tmux") {
        return Some("screen/tmux");
    }
    None
}

/// Should `auto` draw sixel here?
///
/// Sixel works correctly in a bare terminal. Inside zellij it does not:
/// zellij-org/zellij#3372 (open since 0.35.0) renders sixel at double height,
/// and nothing the terminal reports reveals that, so the image overflows its
/// reserved area and bleeds across panes. Half-blocks are always correctly
/// sized, so a multiplexer gets those unless the user has explicitly pinned a
/// cell size to compensate.
pub fn sixel_recommended(source: CellSource) -> bool {
    if !terminal_supports_sixel() || !source.is_trustworthy() {
        return false;
    }
    match multiplexer() {
        // A pinned size means the user has tuned it themselves.
        Some(_) => source == CellSource::Config,
        None => true,
    }
}

/// Whether this terminal is known to render sixel. There is a DA1 query that
/// would answer definitively, but matching the environment covers the common
/// terminals and the config can always override.
pub fn terminal_supports_sixel() -> bool {
    if let Ok(p) = std::env::var("TERM_PROGRAM") {
        let p = p.to_ascii_lowercase();
        if ["wezterm", "mlterm", "contour", "iterm.app"]
            .iter()
            .any(|k| p.contains(k))
        {
            return true;
        }
    }
    let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    ["sixel", "foot", "mlterm", "yaft", "contour"]
        .iter()
        .any(|k| term.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbImage {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, Rgb(c));
            }
        }
        img
    }

    fn raster_dims(s: &str) -> (u32, u32) {
        let start = s.find('"').unwrap() + 1;
        let rest = &s[start..];
        let end = rest.find('#').unwrap();
        let parts: Vec<&str> = rest[..end].split(';').collect();
        (parts[2].parse().unwrap(), parts[3].parse().unwrap())
    }

    #[test]
    fn payload_is_wrapped_in_the_sixel_introducer_and_terminator() {
        let s = encode(&solid(12, 12, [255, 0, 0]));
        assert!(s.starts_with("\x1bP0;0;0q"), "bad introducer");
        assert!(s.ends_with("\x1b\\"), "bad terminator");
        // Must not contain anything that would disturb the surrounding TUI.
        assert!(!s.contains("\x1b[2J"), "must not clear the screen");
        assert!(!s.contains("\x1b[H"), "must not home the cursor");
    }

    #[test]
    fn raster_attributes_state_the_real_pixel_size() {
        let s = encode(&solid(37, 19, [10, 200, 40]));
        assert_eq!(raster_dims(&s), (37, 19));
    }

    #[test]
    fn a_solid_image_uses_exactly_one_colour_register() {
        let s = encode(&solid(24, 12, [255, 255, 255]));
        let regs: Vec<&str> = s.matches(";2;").collect();
        assert_eq!(regs.len(), 1, "solid white should define one register");
        // An adaptive palette reproduces solid white exactly.
        assert!(s.contains("#0;2;100;100;100"), "got {}", &s[..80.min(s.len())]);
    }

    #[test]
    fn an_arbitrary_solid_colour_is_reproduced_exactly() {
        // A fixed colour cube would round this to the nearest cube entry; an
        // adaptive palette should keep it.
        let s = encode(&solid(12, 6, [137, 42, 200]));
        let pct = |v: u32| (v * 100 + 127) / 255;
        let expected = format!("#0;2;{};{};{}", pct(137), pct(42), pct(200));
        assert!(s.contains(&expected), "expected {expected}");
    }

    #[test]
    fn full_height_bands_use_the_all_bits_sixel_char() {
        // 6 rows of one colour = every bit set = 0x3F + 63 = '~'.
        let s = encode(&solid(8, 6, [0, 0, 0]));
        assert!(s.contains('~'), "expected a full-band char");
    }

    #[test]
    fn runs_are_length_encoded() {
        let s = encode(&solid(100, 6, [0, 0, 0]));
        // 100 identical columns must compress rather than repeat 100 chars.
        assert!(s.contains("!100~"), "expected RLE, got {} bytes", s.len());
        assert!(s.len() < 200, "payload should be tiny, got {}", s.len());
    }

    #[test]
    fn short_runs_are_written_literally() {
        let mut img = RgbImage::new(4, 6);
        // Alternate columns so runs stay at length 1.
        for y in 0..6 {
            for x in 0..4 {
                let c = if x % 2 == 0 { [0, 0, 0] } else { [255, 255, 255] };
                img.put_pixel(x, y, Rgb(c));
            }
        }
        let s = encode(&img);
        assert!(!s.contains("!1~"), "single pixels should not use RLE");
    }

    #[test]
    fn band_count_matches_image_height() {
        // 13 rows -> ceil(13/6) = 3 bands -> 2 separators.
        let s = encode(&solid(4, 13, [80, 80, 80]));
        assert_eq!(s.matches('-').count(), 2, "expected 2 band separators");
    }

    #[test]
    fn empty_image_yields_no_payload() {
        assert!(encode(&RgbImage::new(0, 0)).is_empty());
    }

    #[test]
    fn a_gradient_spends_palette_entries_on_the_range_it_covers() {
        let mut img = RgbImage::new(64, 12);
        for y in 0..12 {
            for x in 0..64 {
                let v = (x * 4) as u8;
                img.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        let s = encode(&img);
        let regs = s.matches(";2;").count();
        assert!(regs > 8, "a gradient should use many registers, got {regs}");
        assert!(regs <= PALETTE_SIZE, "must not exceed the palette");
    }

    #[test]
    fn median_cut_splits_distinct_clusters() {
        // Two tight clusters and room for plenty of entries: the palette must
        // contain something near each cluster.
        let mut samples = vec![[10, 10, 10]; 100];
        samples.extend(vec![[240, 30, 30]; 100]);
        let pal = median_cut(&mut samples, 8);
        assert!(pal.iter().any(|c| c[0] < 60 && c[1] < 60), "missing dark cluster");
        assert!(
            pal.iter().any(|c| c[0] > 180 && c[1] < 90),
            "missing red cluster, got {pal:?}"
        );
    }

    #[test]
    fn median_cut_never_exceeds_the_requested_size() {
        let mut samples: Vec<[u8; 3]> = (0..500u32)
            .map(|i| [(i % 256) as u8, ((i * 7) % 256) as u8, ((i * 13) % 256) as u8])
            .collect();
        let pal = median_cut(&mut samples, 16);
        assert!(pal.len() <= 16, "got {}", pal.len());
        assert!(!pal.is_empty());
    }

    #[test]
    fn median_cut_handles_a_single_colour_and_empty_input() {
        let mut one = vec![[7, 8, 9]; 50];
        let pal = median_cut(&mut one, 32);
        assert_eq!(pal.len(), 1, "one colour needs one entry");
        assert_eq!(pal[0], [7, 8, 9]);

        let mut none: Vec<[u8; 3]> = Vec::new();
        assert_eq!(median_cut(&mut none, 8).len(), 1, "must not return empty");
    }

    #[test]
    fn lut_maps_a_colour_to_its_nearest_palette_entry() {
        let pal = vec![[0, 0, 0], [255, 255, 255], [255, 0, 0]];
        let lut = build_lut(&pal);
        assert_eq!(lut[lut_index(&[8, 8, 8])], 0, "near-black -> black");
        assert_eq!(lut[lut_index(&[250, 250, 250])], 1, "near-white -> white");
        assert_eq!(lut[lut_index(&[230, 20, 20])], 2, "near-red -> red");
    }

    #[test]
    #[ignore]
    fn measure_real_cover_payload() {
        // Run with: cargo test measure_real_cover -- --ignored --nocapture
        let Some(dir) = dirs::cache_dir().map(|d| d.join("ytkew")) else {
            eprintln!("no cache dir");
            return;
        };
        let Some(f) = std::fs::read_dir(&dir).ok().and_then(|mut d| {
            d.find_map(|e| e.ok().map(|e| e.path()))
        }) else {
            eprintln!("no cached cover to measure");
            return;
        };
        // Cache files have no extension, so sniff the format from bytes.
        let bytes = std::fs::read(&f).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        let resized = image::imageops::resize(
            &img.to_rgb8(), 320, 320, image::imageops::FilterType::Lanczos3);
        let out = encode(&resized);

        let passes = out.matches('#').count();
        let bands = out.matches('-').count() + 1;
        let regs = out.matches(";2;").count();
        println!("payload  : {} bytes", out.len());
        println!("registers: {regs}");
        println!("bands    : {bands}");
        println!("passes   : {passes}  ({:.1} per band)", passes as f32 / bands as f32);
        println!("bytes/pass: {:.1}", out.len() as f32 / passes as f32);
        let rle = out.matches('!').count();
        println!("rle runs : {rle}");
    }

    #[test]
    fn detected_cell_size_is_always_plausible() {
        let ((w, h), _src) = detect_cell_size(None);
        assert!(plausible(w, h), "got {w}x{h}");
    }

    #[test]
    fn config_cell_size_wins_over_detection() {
        let ((w, h), src) = detect_cell_size(Some((11, 23)));
        assert_eq!((w, h), (11, 23));
        assert_eq!(src, CellSource::Config);
        assert!(src.is_trustworthy());
    }

    #[test]
    fn zero_config_cell_size_falls_through_to_detection() {
        let (_, src) = detect_cell_size(Some((0, 0)));
        assert_ne!(src, CellSource::Config);
    }

    #[test]
    fn implausible_cell_sizes_are_rejected() {
        // Multiplexers can report the outer window's pixels with a pane's
        // cell count, which yields nonsense like a 40px-wide cell.
        assert!(!plausible(40, 25), "absurdly wide cell must be rejected");
        assert!(!plausible(8, 4), "cell shorter than it is wide");
        assert!(!plausible(2, 8), "cell too narrow");
        assert!(!plausible(16, 100), "cell too tall");
        // Real-world values must pass.
        assert!(plausible(8, 16));
        assert!(plausible(16, 32));
        assert!(plausible(12, 24));
        assert!(plausible(10, 21));
    }

    #[test]
    fn only_pinned_or_measured_cell_sizes_are_trusted() {
        // A reported size that is too large makes the cover overlap the
        // track title, so reports never enable sixel by themselves.
        assert!(!CellSource::Query.is_trustworthy());
        assert!(!CellSource::Ioctl.is_trustworthy());
        assert!(!CellSource::Fallback.is_trustworthy());
        assert!(CellSource::Config.is_trustworthy());
        assert!(CellSource::Calibrated.is_trustworthy());
    }

    #[test]
    fn a_multiplexer_only_gets_sixel_when_the_size_is_pinned() {
        // Guards the zellij double-height regression: a detected size is
        // fine in a bare terminal but must not enable sixel under a
        // multiplexer, where it renders at the wrong scale.
        let in_mux = multiplexer().is_some();
        if in_mux {
            assert!(!sixel_recommended(CellSource::Calibrated));
            assert!(!sixel_recommended(CellSource::Query));
        }
        // A guessed size never qualifies, multiplexer or not.
        assert!(!sixel_recommended(CellSource::Fallback));
        assert!(!sixel_recommended(CellSource::Ioctl));
    }

    #[test]
    fn parses_a_cursor_position_report() {
        // CSI <row> ; <col> R
        assert_eq!(parse_cursor_row(b"\x1b[13;1R"), Some(13));
        assert_eq!(parse_cursor_row(b"\x1b[1;1R"), Some(1));
        // Takes the last report if several arrived.
        assert_eq!(parse_cursor_row(b"\x1b[3;1R\x1b[15;1R"), Some(15));
    }

    #[test]
    fn rejects_malformed_cursor_reports() {
        assert_eq!(parse_cursor_row(b""), None);
        assert_eq!(parse_cursor_row(b"\x1b[13;1"), None, "unterminated");
        assert_eq!(parse_cursor_row(b"garbage"), None);
    }

    #[test]
    fn parses_a_cell_size_report() {
        // CSI 6 ; height ; width t
        assert_eq!(parse_cell_report(b"\x1b[6;32;16t"), Some((16, 32)));
        // Tolerates surrounding noise.
        assert_eq!(parse_cell_report(b"junk\x1b[6;21;10tmore"), Some((10, 21)));
    }

    #[test]
    fn rejects_malformed_or_missing_cell_reports() {
        assert_eq!(parse_cell_report(b""), None);
        assert_eq!(parse_cell_report(b"\x1b[6;32"), None, "unterminated");
        assert_eq!(parse_cell_report(b"\x1b[4;953;1383t"), None, "wrong report type");
        assert_eq!(parse_cell_report(b"\x1b[6;0;0t"), None, "zero size");
    }
}
