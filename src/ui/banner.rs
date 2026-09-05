//! The ytkew wordmark, in the heavy block style btop uses for its logo.
//!
//! btop's banner is the "ANSI Shadow" figlet font -- solid `█` blocks with
//! `╗╝║═` shadowing -- six rows tall, drawn in a colour ramp that darkens
//! down the rows (`#E62525` through `#801414` to black). This is the same
//! construction, but the ramp is taken from the active theme rather than
//! being hard-coded red, so it follows the palette like everything else.

use crate::palette::{Palette, Rgb};

/// Rows in the banner, including the shadow row.
pub const ROWS: usize = 6;

const Y: [&str; ROWS] = [
    "██╗   ██╗",
    "╚██╗ ██╔╝",
    " ╚████╔╝ ",
    "  ╚██╔╝  ",
    "   ██║   ",
    "   ╚═╝   ",
];
const T: [&str; ROWS] = [
    "████████╗",
    "╚══██╔══╝",
    "   ██║   ",
    "   ██║   ",
    "   ██║   ",
    "   ╚═╝   ",
];
const K: [&str; ROWS] = [
    "██╗  ██╗",
    "██║ ██╔╝",
    "█████╔╝ ",
    "██╔═██╗ ",
    "██║  ██╗",
    "╚═╝  ╚═╝",
];
const E: [&str; ROWS] = [
    "███████╗",
    "██╔════╝",
    "█████╗  ",
    "██╔══╝  ",
    "███████╗",
    "╚══════╝",
];
const W: [&str; ROWS] = [
    "██╗    ██╗",
    "██║    ██║",
    "██║ █╗ ██║",
    "██║███╗██║",
    "╚███╔███╔╝",
    " ╚══╝╚══╝ ",
];

const LETTERS: [&[&str; ROWS]; 5] = [&Y, &T, &K, &E, &W];

/// The wordmark as six rows of equal width.
pub fn rows() -> [String; ROWS] {
    let mut out: [String; ROWS] = Default::default();
    for letter in LETTERS {
        for (i, row) in out.iter_mut().enumerate() {
            row.push_str(letter[i]);
        }
    }
    out
}

/// Display width of the wordmark.
pub fn width() -> usize {
    LETTERS
        .iter()
        .map(|l| l[0].chars().count())
        .sum()
}

/// Colour ramp down the rows: brightest at the top, fading into the border
/// colour at the shadow row, which is what gives btop's logo its depth.
pub fn shades(palette: &Palette) -> Vec<Rgb> {
    let top = palette.accent();
    let bottom = palette.dark;
    (0..ROWS)
        .map(|i| {
            // Ease toward the dark end so the top rows stay saturated and
            // only the shadow row drops away, as in btop's ramp.
            let t = (i as f32 / (ROWS - 1) as f32).powf(1.6);
            Rgb(
                (top.0 as f32 + (bottom.0 as f32 - top.0 as f32) * t) as u8,
                (top.1 as f32 + (bottom.1 as f32 - top.1 as f32) * t) as u8,
                (top.2 as f32 + (bottom.2 as f32 - top.2 as f32) * t) as u8,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_letter_is_rectangular() {
        for (i, letter) in LETTERS.iter().enumerate() {
            let w = letter[0].chars().count();
            for (r, row) in letter.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    w,
                    "letter {i} row {r} is {} wide, expected {w}",
                    row.chars().count()
                );
            }
        }
    }

    #[test]
    fn the_wordmark_rows_all_match_the_stated_width() {
        let r = rows();
        let w = width();
        assert!(w > 30, "should be a big banner, got {w}");
        for (i, row) in r.iter().enumerate() {
            assert_eq!(row.chars().count(), w, "row {i} is the wrong width");
        }
    }

    #[test]
    fn the_ramp_darkens_from_top_to_bottom() {
        let p = Palette::default();
        let s = shades(&p);
        assert_eq!(s.len(), ROWS);
        let lum = |c: Rgb| 0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32;
        for i in 1..ROWS {
            assert!(
                lum(s[i]) <= lum(s[i - 1]) + 0.5,
                "row {i} is brighter than the one above it"
            );
        }
        assert!(lum(s[0]) > lum(s[ROWS - 1]), "top should outshine the shadow");
    }

    #[test]
    fn the_banner_is_solid_blocks_not_line_drawing() {
        // The heavy look comes from full blocks; line-drawing alone would be
        // the small font again.
        let joined = rows().join("");
        assert!(joined.contains('█'), "expected solid blocks");
        assert!(joined.contains('╗') && joined.contains('╝'), "expected shadowing");
    }
}
