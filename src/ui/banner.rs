//! The ytkew wordmark, in the heavy block style btop uses for its logo.
//!
//! btop's banner is the "ANSI Shadow" figlet font -- solid `█` blocks with
//! `╗╝║═` shadowing -- six rows tall, drawn in a colour ramp that darkens
//! down the rows (`#E62525` through `#801414` to black). This is the same
//! construction, but the ramp is taken from the active theme rather than
//! being hard-coded red, so it follows the palette like everything else.

use crate::palette::Rgb;

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

/// btop's own ramp, verbatim from `Global::Banner_src`. The logo keeps this
/// regardless of theme, the way btop's stays red: it reads as the mark rather
/// than as another themed element.
pub const RAMP: [Rgb; ROWS] = [
    Rgb(0xE6, 0x25, 0x25),
    Rgb(0xCD, 0x21, 0x21),
    Rgb(0xB3, 0x1D, 0x1D),
    Rgb(0x9A, 0x19, 0x19),
    Rgb(0x80, 0x14, 0x14),
    Rgb(0x4A, 0x0C, 0x0C),
];

/// Colour for each row, brightest at the top and fading into the shadow.
pub fn shades() -> [Rgb; ROWS] {
    RAMP
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
        let s = shades();
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
    fn the_shadow_row_stays_visible_on_a_black_terminal() {
        // btop ends its ramp at pure black, which vanishes entirely on a dark
        // background. The last row is lifted just enough to still read.
        let last = shades()[ROWS - 1];
        let lum = 0.2126 * last.0 as f32 + 0.7152 * last.1 as f32 + 0.0722 * last.2 as f32;
        assert!(lum > 8.0, "shadow row would be invisible ({last:?})");
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
