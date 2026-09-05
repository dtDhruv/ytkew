//! Named colour themes, built in and user supplied.
//!
//! A theme is three colours, matching the slots the palette already exposes:
//! `dark` for borders and inactive chrome, `mid` for secondary text, `bright`
//! for accents, titles and the progress bar. That is deliberately the same
//! shape the cover-derived palette produces, so a fixed theme and album
//! colours are interchangeable everywhere.
//!
//! Users add their own by dropping a TOML file into `themes/` beside the
//! config:
//!
//! ```toml
//! # ~/.config/ytkew/themes/ayu.toml
//! dark   = "#3d4751"
//! mid    = "#b3b1ad"
//! bright = "#ffcc66"
//! ```
//!
//! The file name is the theme name. A user theme that reuses a built-in name
//! replaces it, so any shipped theme can be retuned without patching ytkew.

use crate::palette::{Palette, Rgb};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
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

/// A theme as written in a user's TOML file.
#[derive(Deserialize)]
struct ThemeFile {
    /// Defaults to the file name, which is how nearly everyone will use it.
    name: Option<String>,
    dark: String,
    mid: String,
    bright: String,
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb(r, g, b)
}

/// Name and colours of a shipped theme, in the form a `const` can hold.
struct BuiltIn {
    name: &'static str,
    dark: Rgb,
    mid: Rgb,
    bright: Rgb,
}

const BUILT_IN: &[BuiltIn] = &[
    BuiltIn {
        name: "gruvbox",
        dark: rgb(0x50, 0x49, 0x45),
        mid: rgb(0xd5, 0xc4, 0xa1),
        bright: rgb(0xfa, 0xbd, 0x2f),
    },
    BuiltIn {
        name: "nord",
        dark: rgb(0x4c, 0x56, 0x6a),
        mid: rgb(0x81, 0xa1, 0xc1),
        bright: rgb(0x88, 0xc0, 0xd0),
    },
    BuiltIn {
        name: "dracula",
        dark: rgb(0x44, 0x47, 0x5a),
        mid: rgb(0x62, 0x72, 0xa4),
        bright: rgb(0xbd, 0x93, 0xf9),
    },
    BuiltIn {
        name: "catppuccin",
        dark: rgb(0x45, 0x47, 0x5a),
        mid: rgb(0xa6, 0xad, 0xc8),
        bright: rgb(0xcb, 0xa6, 0xf7),
    },
    BuiltIn {
        name: "tokyonight",
        dark: rgb(0x41, 0x48, 0x68),
        mid: rgb(0x7a, 0xa2, 0xf7),
        bright: rgb(0xbb, 0x9a, 0xf7),
    },
    BuiltIn {
        name: "everforest",
        dark: rgb(0x4f, 0x58, 0x5e),
        mid: rgb(0xa7, 0xc0, 0x80),
        bright: rgb(0xdb, 0xbc, 0x7f),
    },
    BuiltIn {
        name: "rosepine",
        dark: rgb(0x52, 0x4f, 0x67),
        mid: rgb(0x9c, 0xcf, 0xd8),
        bright: rgb(0xeb, 0xbc, 0xba),
    },
    BuiltIn {
        name: "solarized",
        dark: rgb(0x07, 0x36, 0x42),
        mid: rgb(0x83, 0x94, 0x96),
        bright: rgb(0x26, 0x8b, 0xd2),
    },
    BuiltIn {
        name: "matrix",
        dark: rgb(0x0f, 0x3d, 0x0f),
        mid: rgb(0x2f, 0xa8, 0x2f),
        bright: rgb(0x5c, 0xff, 0x5c),
    },
    BuiltIn {
        name: "mono",
        dark: rgb(0x44, 0x44, 0x44),
        mid: rgb(0x99, 0x99, 0x99),
        bright: rgb(0xee, 0xee, 0xee),
    },
];

/// The theme name meaning "use the album artwork".
pub const COVER: &str = "cover";

/// Where user themes live, relative to the config directory.
pub const THEME_DIR: &str = "themes";

/// The themes available this run: everything shipped, plus whatever the user
/// dropped in `themes/`.
#[derive(Clone, Debug)]
pub struct Themes {
    themes: Vec<Theme>,
}

impl Default for Themes {
    fn default() -> Self {
        Self {
            themes: BUILT_IN
                .iter()
                .map(|t| Theme {
                    name: t.name.to_string(),
                    dark: t.dark,
                    mid: t.mid,
                    bright: t.bright,
                })
                .collect(),
        }
    }
}

impl Themes {
    /// The built-ins plus every readable theme in `<config_dir>/themes`.
    ///
    /// Returns any complaints alongside the registry rather than failing: one
    /// malformed file should cost the user that theme, not the whole app.
    pub fn load(config_dir: &Path) -> (Self, Vec<String>) {
        let mut out = Self::default();
        let mut problems = Vec::new();
        let dir = config_dir.join(THEME_DIR);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // No themes directory is the normal case, not a problem.
            return (out, problems);
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        // Load in a stable order so the result does not depend on the
        // filesystem's iteration order.
        files.sort();
        for path in files {
            match parse_theme_file(&path) {
                Ok(theme) => out.insert(theme),
                Err(e) => problems.push(format!(
                    "{}: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }
        (out, problems)
    }

    /// Add a theme, replacing any built-in of the same name so a user can
    /// retune a shipped theme in place.
    fn insert(&mut self, theme: Theme) {
        match self.themes.iter_mut().find(|t| t.name == theme.name) {
            Some(existing) => *existing = theme,
            None => self.themes.push(theme),
        }
    }

    /// Every selectable name, with `cover` first since it is the default.
    pub fn names(&self) -> Vec<String> {
        let mut v = vec![COVER.to_string()];
        v.extend(self.themes.iter().map(|t| t.name.clone()));
        v
    }

    /// Look up a theme by name, case-insensitively. `None` for `cover` or an
    /// unknown name, both of which mean "no fixed palette".
    pub fn find(&self, name: &str) -> Option<&Theme> {
        let n = name.trim().to_ascii_lowercase();
        self.themes.iter().find(|t| t.name == n)
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }
}

/// Create `themes/` with a note on the format, so the feature is visible
/// without reading the docs. Never touches an existing directory.
pub fn write_example_if_missing(config_dir: &Path) -> std::io::Result<()> {
    let dir = config_dir.join(THEME_DIR);
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    // .txt rather than .toml: only .toml files are loaded, so this cannot
    // turn into a phantom theme in the picker.
    std::fs::write(dir.join("README.txt"), EXAMPLE)
}

const EXAMPLE: &str = "\
Drop a .toml file in this directory and it becomes a theme. The file name is
the theme name, so `ayu.toml` gives you a theme called `ayu`, selectable from
the options pane (esc -> options -> theme).

A theme is three colours:

    dark   = \"#3d4751\"   # borders and inactive chrome
    mid    = \"#b3b1ad\"   # secondary text
    bright = \"#ffcc66\"   # accents, titles, the progress bar

Both `#rrggbb` and `rrggbb` are accepted. Add `name = \"...\"` to use a name
other than the file name.

Reusing a built-in name (gruvbox, nord, dracula, catppuccin, tokyonight,
everforest, rosepine, solarized, matrix, mono) replaces that theme, so you can
retune a shipped one without patching ytkew.

`cover` is reserved: it means \"take the colours from the album art\".

Themes are read at startup, so restart ytkew after adding one. A file that
does not parse is reported on startup and skipped -- the rest still load.
";

fn parse_theme_file(path: &Path) -> Result<Theme, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let file: ThemeFile = toml::from_str(&raw).map_err(|e| e.message().to_string())?;
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let name = file.name.unwrap_or(stem).trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("theme name is empty".into());
    }
    if name == COVER {
        return Err(format!("`{COVER}` is reserved for the album palette"));
    }
    let colour = |label: &str, raw: &str| {
        parse_hex(raw).ok_or_else(|| format!("{label} is not a #rrggbb colour: {raw:?}"))
    };
    Ok(Theme {
        name,
        dark: colour("dark", &file.dark)?,
        mid: colour("mid", &file.mid)?,
        bright: colour("bright", &file.bright)?,
    })
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

    fn built_ins() -> Vec<Theme> {
        Themes::default().themes
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir.join(THEME_DIR)).unwrap();
        std::fs::write(dir.join(THEME_DIR).join(name), body).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ytkew-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_user_theme_is_picked_up_and_named_after_its_file() {
        let dir = tmp("user");
        write(
            &dir,
            "ayu.toml",
            "dark=\"#3d4751\"\nmid=\"#b3b1ad\"\nbright=\"#ffcc66\"\n",
        );
        let (themes, problems) = Themes::load(&dir);
        assert!(problems.is_empty(), "{problems:?}");
        let t = themes.find("ayu").expect("ayu should load");
        assert_eq!(t.bright, Rgb(0xff, 0xcc, 0x66));
        assert!(themes.names().contains(&"ayu".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_user_theme_replaces_a_built_in_of_the_same_name() {
        let dir = tmp("override");
        write(
            &dir,
            "nord.toml",
            "dark=\"#000000\"\nmid=\"#777777\"\nbright=\"#ffffff\"\n",
        );
        let (themes, problems) = Themes::load(&dir);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(themes.find("nord").unwrap().bright, Rgb(255, 255, 255));
        // Replaced, not added alongside.
        assert_eq!(themes.len(), built_ins().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_bad_theme_file_does_not_cost_the_others() {
        let dir = tmp("bad");
        write(
            &dir,
            "good.toml",
            "dark=\"#111111\"\nmid=\"#888888\"\nbright=\"#eeeeee\"\n",
        );
        write(
            &dir,
            "broken.toml",
            "dark=\"not a colour\"\nmid=\"#888888\"\nbright=\"#eeeeee\"\n",
        );
        write(&dir, "truncated.toml", "dark=\"#111111\"\n");
        let (themes, problems) = Themes::load(&dir);
        assert!(themes.find("good").is_some(), "the valid theme still loads");
        assert!(themes.find("broken").is_none());
        assert_eq!(problems.len(), 2, "both failures reported: {problems:?}");
        assert!(problems.iter().any(|p| p.contains("broken.toml")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_theme_cannot_hijack_the_cover_name() {
        let dir = tmp("cover");
        write(
            &dir,
            "cover.toml",
            "dark=\"#111111\"\nmid=\"#888888\"\nbright=\"#eeeeee\"\n",
        );
        let (themes, problems) = Themes::load(&dir);
        assert_eq!(themes.len(), built_ins().len());
        assert!(problems[0].contains("reserved"), "{problems:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_shipped_note_is_not_mistaken_for_a_theme() {
        let dir = tmp("note");
        write_example_if_missing(&dir).unwrap();
        let (themes, problems) = Themes::load(&dir);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(themes.len(), built_ins().len(), "README.txt is not a theme");
        // And it never clobbers a directory the user already has.
        write(
            &dir,
            "ayu.toml",
            "dark=\"#111111\"\nmid=\"#888888\"\nbright=\"#eeeeee\"\n",
        );
        write_example_if_missing(&dir).unwrap();
        assert!(Themes::load(&dir).0.find("ayu").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_themes_directory_is_not_an_error() {
        let dir = tmp("empty");
        let (themes, problems) = Themes::load(&dir);
        assert!(problems.is_empty());
        assert_eq!(themes.len(), built_ins().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_theme_reads_dark_to_bright() {
        // The slots feed borders, secondary text and accents in that order,
        // so a theme whose "bright" is darker than its "dark" would invert
        // the whole interface.
        for t in built_ins() {
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
        for t in built_ins() {
            let a = t.palette().accent();
            let lum = 0.2126 * a.0 as f32 + 0.7152 * a.1 as f32 + 0.0722 * a.2 as f32;
            assert!(lum > 70.0, "{}: accent too dim ({a:?})", t.name);
        }
    }

    #[test]
    fn names_are_unique_and_lowercase() {
        let mut seen = Vec::new();
        for n in Themes::default().names() {
            assert_eq!(n, n.to_ascii_lowercase(), "{n} should be lowercase");
            assert!(!seen.contains(&n), "duplicate theme {n}");
            seen.push(n);
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_cover_is_not_a_fixed_theme() {
        let t = Themes::default();
        assert!(t.find("Gruvbox").is_some());
        assert!(t.find("  NORD ").is_some());
        assert!(t.find("cover").is_none(), "cover means the album palette");
        assert!(t.find("nonsense").is_none());
    }

    #[test]
    fn cover_leads_the_list_so_it_is_the_first_choice() {
        let all = Themes::default().names();
        assert_eq!(all[0], COVER);
        assert_eq!(all.len(), built_ins().len() + 1);
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
