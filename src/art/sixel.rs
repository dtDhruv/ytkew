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

use super::quantize::{build_lut, lut_index, median_cut, PALETTE_SIZE};
use image::RgbImage;
use std::fmt::Write as _;

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
        assert!(
            s.contains("#0;2;100;100;100"),
            "got {}",
            &s[..80.min(s.len())]
        );
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
                let c = if x % 2 == 0 {
                    [0, 0, 0]
                } else {
                    [255, 255, 255]
                };
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
        assert!(
            pal.iter().any(|c| c[0] < 60 && c[1] < 60),
            "missing dark cluster"
        );
        assert!(
            pal.iter().any(|c| c[0] > 180 && c[1] < 90),
            "missing red cluster, got {pal:?}"
        );
    }

    #[test]
    fn median_cut_never_exceeds_the_requested_size() {
        let mut samples: Vec<[u8; 3]> = (0..500u32)
            .map(|i| {
                [
                    (i % 256) as u8,
                    ((i * 7) % 256) as u8,
                    ((i * 13) % 256) as u8,
                ]
            })
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
        let Some(f) = std::fs::read_dir(&dir)
            .ok()
            .and_then(|mut d| d.find_map(|e| e.ok().map(|e| e.path())))
        else {
            eprintln!("no cached cover to measure");
            return;
        };
        // Cache files have no extension, so sniff the format from bytes.
        let bytes = std::fs::read(&f).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        let resized = image::imageops::resize(
            &img.to_rgb8(),
            320,
            320,
            image::imageops::FilterType::Lanczos3,
        );
        let out = encode(&resized);

        let passes = out.matches('#').count();
        let bands = out.matches('-').count() + 1;
        let regs = out.matches(";2;").count();
        println!("payload  : {} bytes", out.len());
        println!("registers: {regs}");
        println!("bands    : {bands}");
        println!(
            "passes   : {passes}  ({:.1} per band)",
            passes as f32 / bands as f32
        );
        println!("bytes/pass: {:.1}", out.len() as f32 / passes as f32);
        let rle = out.matches('!').count();
        println!("rle runs : {rle}");
    }
}
