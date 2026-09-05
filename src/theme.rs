//! Named colour themes.
//!
//! A theme is three colours, matching the slots the palette already exposes:
//! `dark` for borders and inactive chrome, `mid` for secondary text, `bright`
//! for accents, titles and the progress bar. That is deliberately the same
//! shape the cover-derived palette produces, so a fixed theme and album
//! colours are interchangeable everywhere.

use crate::palette::{Palette, Rgb};

pub struct Theme {
    pub name: &'static str,
    pub dark: Rgb,
    pub mid: Rgb,
    pub bright: Rgb,
}

impl Theme {
    pub fn palette(&self) -> Palette {
        Palette {
            dark: self.dark,
            mid: self.mid,
            bright: self.bright,
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb(r, g, b)
}

/// The built-in themes. `cover` is handled separately -- it means "take the
/// colours from the album art" and so has no fixed values here.
pub const THEMES: &[Theme] = &[
    Theme {
        name: "gruvbox",
        dark: rgb(0x50, 0x49, 0x45),
        mid: rgb(0xd5, 0xc4, 0xa1),
        bright: rgb(0xfa, 0xbd, 0x2f),
    },
    Theme {
        name: "nord",
        dark: rgb(0x4c, 0x56, 0x6a),
        mid: rgb(0x81, 0xa1, 0xc1),
        bright: rgb(0x88, 0xc0, 0xd0),
    },
    Theme {
        name: "dracula",
        dark: rgb(0x44, 0x47, 0x5a),
        mid: rgb(0x62, 0x72, 0xa4),
        bright: rgb(0xbd, 0x93, 0xf9),
    },
    Theme {
        name: "catppuccin",
        dark: rgb(0x45, 0x47, 0x5a),
        mid: rgb(0xa6, 0xad, 0xc8),
        bright: rgb(0xcb, 0xa6, 0xf7),
    },
    Theme {
        name: "tokyonight",
        dark: rgb(0x41, 0x48, 0x68),
        mid: rgb(0x7a, 0xa2, 0xf7),
        bright: rgb(0xbb, 0x9a, 0xf7),
    },
    Theme {
        name: "everforest",
        dark: rgb(0x4f, 0x58, 0x5e),
        mid: rgb(0xa7, 0xc0, 0x80),
        bright: rgb(0xdb, 0xbc, 0x7f),
    },
    Theme {
        name: "rosepine",
        dark: rgb(0x52, 0x4f, 0x67),
        mid: rgb(0x9c, 0xcf, 0xd8),
        bright: rgb(0xeb, 0xbc, 0xba),
    },
    Theme {
        name: "solarized",
        dark: rgb(0x07, 0x36, 0x42),
        mid: rgb(0x83, 0x94, 0x96),
        bright: rgb(0x26, 0x8b, 0xd2),
    },
    Theme {
        name: "matrix",
        dark: rgb(0x0f, 0x3d, 0x0f),
        mid: rgb(0x2f, 0xa8, 0x2f),
        bright: rgb(0x5c, 0xff, 0x5c),
    },
    Theme {
        name: "mono",
        dark: rgb(0x44, 0x44, 0x44),
        mid: rgb(0x99, 0x99, 0x99),
        bright: rgb(0xee, 0xee, 0xee),
    },
];

/// The theme name meaning "use the album artwork".
pub const COVER: &str = "cover";

/// Every selectable name, with `cover` first since it is the default.
pub fn names() -> Vec<&'static str> {
    let mut v = vec![COVER];
    v.extend(THEMES.iter().map(|t| t.name));
    v
}

/// Look up a theme by name, case-insensitively. `None` for `cover` or an
/// unknown name, both of which mean "no fixed palette".
pub fn find(name: &str) -> Option<&'static Theme> {
    let n = name.trim().to_ascii_lowercase();
    THEMES.iter().find(|t| t.name == n)
}

/// Parse `#rrggbb` or `rrggbb`.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Rgb(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

/// A palette from three hex strings, for a theme defined in the config.
pub fn from_hex(colors: &[String]) -> Option<Palette> {
    if colors.len() != 3 {
        return None;
    }
    Some(Palette {
        dark: parse_hex(&colors[0])?,
        mid: parse_hex(&colors[1])?,
        bright: parse_hex(&colors[2])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_reads_dark_to_bright() {
        // The slots feed borders, secondary text and accents in that order,
        // so a theme whose "bright" is darker than its "dark" would invert
        // the whole interface.
        for t in THEMES {
            let p = t.palette();
            let lum = |c: Rgb| 0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32;
            assert!(
                lum(p.dark) < lum(p.bright),
                "{}: dark {:?} is not darker than bright {:?}",
                t.name,
                p.dark,
                p.bright
            );
        }
    }

    #[test]
    fn every_theme_has_a_visible_accent() {
        for t in THEMES {
            let a = t.palette().accent();
            let lum = 0.2126 * a.0 as f32 + 0.7152 * a.1 as f32 + 0.0722 * a.2 as f32;
            assert!(lum > 70.0, "{}: accent too dim ({a:?})", t.name);
        }
    }

    #[test]
    fn names_are_unique_and_lowercase() {
        let mut seen = Vec::new();
        for n in names() {
            assert_eq!(n, &n.to_ascii_lowercase(), "{n} should be lowercase");
            assert!(!seen.contains(&n), "duplicate theme {n}");
            seen.push(n);
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_cover_is_not_a_fixed_theme() {
        assert!(find("Gruvbox").is_some());
        assert!(find("  NORD ").is_some());
        assert!(find("cover").is_none(), "cover means the album palette");
        assert!(find("nonsense").is_none());
    }

    #[test]
    fn cover_leads_the_list_so_it_is_the_first_choice() {
        let all = names();
        assert_eq!(all[0], COVER);
        assert_eq!(all.len(), THEMES.len() + 1);
    }

    #[test]
    fn hex_parsing_accepts_both_forms_and_rejects_junk() {
        assert_eq!(parse_hex("#ff8800"), Some(Rgb(0xff, 0x88, 0x00)));
        assert_eq!(parse_hex("ff8800"), Some(Rgb(0xff, 0x88, 0x00)));
        assert_eq!(parse_hex(" #FF8800 "), Some(Rgb(0xff, 0x88, 0x00)));
        assert_eq!(parse_hex("#fff"), None, "short form is not supported");
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn a_custom_palette_needs_exactly_three_colours() {
        let three: Vec<String> = ["#101010", "#808080", "#f0f0f0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(from_hex(&three).is_some());
        assert!(from_hex(&three[..2]).is_none());
        let bad: Vec<String> = ["#101010", "nope", "#f0f0f0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(from_hex(&bad).is_none());
    }
}
