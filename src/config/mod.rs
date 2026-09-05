//! User configuration.
//!
//! This file is read, never written: comments and hand-edits are preserved.
//! Anything the app needs to remember goes in [`state::State`] instead.

pub mod keymap;
pub mod state;

pub use keymap::{Action, Keymap};
pub use state::State;

use serde::{Deserialize, Serialize};
use std::path::Path;

/// How album art is drawn.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CoverMode {
    /// Best available: kitty graphics, then sixel, then half-blocks.
    #[default]
    Auto,
    /// Kitty graphics protocol. Placements are sized in cells, so the
    /// terminal does the scaling and no cell pixel size is needed.
    Kitty,
    /// Sixel. Needs an accurate cell pixel size to scale correctly.
    Sixel,
    /// Truecolor half-blocks: two pixels per cell, works anywhere.
    Blocks,
    Off,
}

impl CoverMode {
    /// Does `auto` resolve to sixel? Only when the user has pinned a cell
    /// size, because no automatic measurement has proven reliable: the
    /// terminal's report, the tty window size and a cursor-advance probe all
    /// disagree with what actually gets drawn under a multiplexer. Blocks are
    /// always correctly sized, so they are the safe default.
    pub fn name(self) -> &'static str {
        match self {
            CoverMode::Auto => "auto",
            CoverMode::Kitty => "kitty",
            CoverMode::Sixel => "sixel",
            CoverMode::Blocks => "blocks",
            CoverMode::Off => "off",
        }
    }

    /// Does this mode draw sixel, given what the terminal can do?
    pub fn uses_sixel(self, terminal_ok: bool) -> bool {
        match self {
            CoverMode::Sixel => true,
            CoverMode::Auto => terminal_ok,
            _ => false,
        }
    }

    /// Does this mode draw kitty graphics?
    pub fn uses_kitty(self, terminal_ok: bool) -> bool {
        match self {
            CoverMode::Kitty => true,
            CoverMode::Auto => terminal_ok,
            _ => false,
        }
    }

    pub fn draws_anything(self) -> bool {
        !matches!(self, CoverMode::Off)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VisualizerMode {
    /// Eighth-block bars: ▁▂▃▄▅▆▇█
    #[default]
    Bars,
    /// Braille dots, denser and half the vertical cost.
    Braille,
    Off,
}

impl VisualizerMode {
    pub fn cycle(self) -> Self {
        match self {
            VisualizerMode::Bars => VisualizerMode::Braille,
            VisualizerMode::Braille => VisualizerMode::Off,
            VisualizerMode::Off => VisualizerMode::Bars,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub visualizer_height: u16,
    pub visualizer_bar_width: u16,
    pub visualizer_mode: VisualizerMode,
    /// Whether art is shown on a first run, before any state is saved.
    /// `b` toggles it thereafter.
    pub cover_enabled: bool,
    pub cover_mode: CoverMode,
    /// Terminal cell size in pixels, as `[width, height]`. Leave at [0, 0] to
    /// detect it; set it only if sixel art comes out the wrong size.
    pub cell_px: [u16; 2],
    pub hide_help: bool,
    /// Volume used on a first run, before any state has been saved.
    pub keys: crate::config::keymap::KeyPreset,
    pub initial_volume: f64,
    pub volume_max: f64,
    pub volume_step: f64,
    pub seek_step: f64,
    /// Theme name: "cover" to take colours from the album art, one of the
    /// built-ins, or "custom" with `theme_colors` below.
    pub theme: String,
    /// Three `#rrggbb` values -- borders, secondary text, accent -- used when
    /// `theme = "custom"`.
    pub theme_colors: Vec<String>,
    /// Superseded by `theme = "cover"`; kept so older configs still load.
    pub color_from_cover: bool,
    /// Fallback accent when there's no cover (kew defaults to ANSI 6, cyan).
    pub accent_color: u8,
    /// Carry shuffle and repeat across restarts.
    pub save_repeat_shuffle: bool,
    /// Append a radio mix when a bare search is played, so one song does not
    /// leave you in silence.
    pub autoplay_radio: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            visualizer_height: 6,
            visualizer_bar_width: 2,
            visualizer_mode: VisualizerMode::Bars,
            cover_enabled: true,
            cover_mode: CoverMode::Auto,
            cell_px: [0, 0],
            hide_help: false,
            keys: crate::config::keymap::KeyPreset::Kew,
            initial_volume: 100.0,
            volume_max: 100.0,
            volume_step: 5.0,
            seek_step: 5.0,
            theme: crate::theme::COVER.to_string(),
            theme_colors: Vec::new(),
            color_from_cover: true,
            accent_color: 6,
            save_repeat_shuffle: false,
            autoplay_radio: true,
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.toml");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&raw) {
            Ok(c) => c,
            Err(e) => {
                // A broken config should not stop the music.
                eprintln!("ytkew: ignoring bad config.toml: {e}");
                Self::default()
            }
        }
    }

    /// Write a commented default config, but only when none exists. The app
    /// never overwrites this file -- it belongs to the user, comments and all.
    pub fn write_default_if_missing(dir: &Path) -> anyhow::Result<()> {
        let path = dir.join("config.toml");
        if path.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(dir)?;
        std::fs::write(path, DEFAULT_CONFIG_TOML)?;
        Ok(())
    }
}

const DEFAULT_CONFIG_TOML: &str = r##"# ytkew configuration. Edit freely -- ytkew only reads this file.
# Runtime state (volume, shuffle, repeat) is kept separately in state.toml.

visualizer_height = 6
visualizer_bar_width = 2
visualizer_mode = "bars"      # bars | braille | off

cover_mode = "auto"           # which renderer to use for album art:
                              #   auto   - kitty if available, else blocks
                              #   kitty  - kitty graphics protocol
                              #   sixel  - sixel (needs an accurate cell_px)
                              #   blocks - truecolor half-blocks, works anywhere
                              # `b` shows and hides the art; it does not
                              # change the renderer. Cycle renderers from the
                              # escape menu.
cell_px = [0, 0]              # cell size in px for sixel. [0,0] = unset.
                              # Easiest way to set it: run ytkew, press `b`
                              # until you see sixel, then `[` and `]` to
                              # resize until it fits. It saves automatically.
cover_enabled = true

# Colours. "cover" takes them from the album art, which is the default and
# what kew does. Or pick a built-in:
#   gruvbox  nord  dracula  catppuccin  tokyonight
#   everforest  rosepine  solarized  matrix  mono
# `t` cycles at runtime and remembers your choice.
theme = "cover"

# With theme = "custom", these three are borders, secondary text, accent.
# theme = "custom"
# theme_colors = ["#504945", "#d5c4a1", "#fabd2f"]

accent_color = 6              # ANSI index, used only as a last resort

keys = "kew"                  # kew | vim -- vim swaps navigation for vim
                              # motions (gg, G, ctrl+d/u, dd, x, J/K) and
                              # moves next/prev track to H/L
initial_volume = 100.0        # only applies before state.toml exists
volume_max = 100.0            # raise to at most 130 to allow boosting quiet
                              # tracks; above 100 mpv adds plain digital gain
                              # with nothing to catch the peaks, so loud
                              # material will clip and sound fuzzy
volume_step = 5.0
seek_step = 5.0

autoplay_radio = true         # append a radio mix behind a played search hit
save_repeat_shuffle = false   # remember shuffle/repeat across restarts
hide_help = false
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_toml() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.visualizer_height, c.visualizer_height);
        assert_eq!(back.accent_color, c.accent_color);
    }

    #[test]
    fn shipped_default_config_actually_parses() {
        // A default file that fails to load would silently fall back, which is
        // exactly the bug this guards.
        let c: Config = toml::from_str(DEFAULT_CONFIG_TOML)
            .expect("the shipped default config must deserialize");
        assert_eq!(c.visualizer_height, 6);
        assert_eq!(c.accent_color, 6);
        assert!(c.color_from_cover);
        assert!(c.autoplay_radio);
    }

    #[test]
    fn partial_config_keeps_defaults_for_everything_else() {
        let c: Config = toml::from_str("accent_color = 1\ncolor_from_cover = false\n").unwrap();
        assert_eq!(c.accent_color, 1);
        assert!(!c.color_from_cover);
        // Untouched fields fall back to the struct default, not zero.
        assert_eq!(c.visualizer_height, 6);
        assert_eq!(c.volume_step, 5.0);
    }

    #[test]
    fn unknown_config_keys_are_ignored_not_fatal() {
        // Fields removed in a later version must not break an old file.
        let c: Config =
            toml::from_str("cover_ansi = true\nhide_logo = true\naccent_color = 2\n").unwrap();
        assert_eq!(c.accent_color, 2);
    }

    #[test]
    fn writing_defaults_never_clobbers_an_existing_config() {
        let dir = std::env::temp_dir().join(format!("ytkew-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "# my comments\naccent_color = 9\n").unwrap();

        Config::write_default_if_missing(&dir).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("# my comments"),
            "user comments must survive"
        );
        assert!(after.contains("accent_color = 9"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
