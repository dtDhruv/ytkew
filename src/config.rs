//! Settings and keybindings.
//!
//! Defaults deliberately mirror kew's, down to the 6-row visualizer and the
//! `h`/`l` track skipping, so muscle memory carries over.

use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Every discrete thing a keypress can do. Mirrors kew's `MSG_*` set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    PlayPause,
    Stop,
    Next,
    Prev,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
    VolumeUp,
    VolumeDown,
    SeekForward,
    SeekBack,
    Shuffle,
    ToggleRepeat,
    Enqueue,
    EnqueueAndPlay,
    Remove,
    ClearQueue,
    MoveUp,
    MoveDown,
    ShowQueue,
    ShowLibrary,
    ShowTrack,
    ShowSearch,
    ShowHelp,
    ShowLyrics,
    NextView,
    PrevView,
    CycleVisualizer,
    ToggleAscii,
    ToggleLike,
    StartRadio,
    /// Play everything in the current context: the selected playlist, album
    /// or artist in the library, or all the search results.
    PlayAll,
    /// Open or close the menu overlay.
    ToggleMenu,
    /// Shrink the assumed cell size, making sixel art smaller.
    CoverSmaller,
    /// Grow the assumed cell size, making sixel art larger.
    CoverBigger,
    Quit,
}

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
    pub initial_volume: f64,
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
            initial_volume: 100.0,
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

initial_volume = 100.0        # only applies before state.toml exists
volume_step = 5.0
seek_step = 5.0

autoplay_radio = true         # append a radio mix behind a played search hit
save_repeat_shuffle = false   # remember shuffle/repeat across restarts
hide_help = false
"##;

/// Things the app owns and rewrites: transport state that should survive a
/// restart. Kept apart from `Config` so user edits are never clobbered.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: String,
    /// Cell size the user tuned by eye with `[`/`]`. Treated as pinned.
    pub cover_cell: [u16; 2],
    /// Whether album art is showing. Toggled with `b` and remembered.
    pub cover_visible: bool,
    /// Theme chosen at runtime with `t`. Empty means "use the config".
    pub theme: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            volume: 100.0,
            shuffle: false,
            repeat: "off".into(),
            cover_cell: [0, 0],
            cover_visible: true,
            theme: String::new(),
        }
    }
}

impl State {
    pub fn load(dir: &Path, cfg: &Config) -> Self {
        match std::fs::read_to_string(dir.join("state.toml")) {
            Ok(raw) => toml::from_str(&raw).unwrap_or_else(|_| State {
                volume: cfg.initial_volume,
                ..Default::default()
            }),
            // First run: take the volume the config asks for.
            Err(_) => State {
                volume: cfg.initial_volume,
                ..Default::default()
            },
        }
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("state.toml"), toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// A resolved key -> action table.
pub struct Keymap {
    bindings: Vec<(KeyCode, KeyModifiers, Action)>,
}

impl Default for Keymap {
    /// kew's default bindings, verbatim where they make sense for a
    /// streaming client.
    fn default() -> Self {
        use Action::*;
        use KeyCode::*;
        const NONE: KeyModifiers = KeyModifiers::NONE;
        const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

        let bindings = vec![
            // Playback
            (Char(' '), NONE, PlayPause),
            (Char('p'), NONE, PlayPause),
            (Char('S'), SHIFT, Stop),
            (Char('h'), NONE, Prev),
            (Char('l'), NONE, Next),
            (Left, NONE, Prev),
            (Right, NONE, Next),
            (Char('a'), NONE, SeekBack),
            (Char('d'), NONE, SeekForward),
            // Volume
            (Char('+'), NONE, VolumeUp),
            (Char('='), NONE, VolumeUp),
            (Char('-'), NONE, VolumeDown),
            // Navigation
            (Char('k'), NONE, ScrollUp),
            (Char('j'), NONE, ScrollDown),
            (Up, NONE, ScrollUp),
            (Down, NONE, ScrollDown),
            (KeyCode::PageUp, NONE, Action::PageUp),
            (KeyCode::PageDown, NONE, Action::PageDown),
            (Home, NONE, Top),
            (End, NONE, Bottom),
            (Tab, NONE, NextView),
            (BackTab, SHIFT, PrevView),
            // Modes
            (Char('s'), NONE, Shuffle),
            (Char('r'), NONE, ToggleRepeat),
            (Char('v'), NONE, CycleVisualizer),
            (Char('b'), NONE, ToggleAscii),
            (Char('.'), NONE, ToggleLike),
            (Char('R'), SHIFT, StartRadio),
            (Char('['), NONE, CoverSmaller),
            (Char(']'), NONE, CoverBigger),
            // Queue editing
            (Enter, NONE, Enqueue),
            // kew separates "add to queue" from "add and jump to it".
            (Enter, KeyModifiers::ALT, EnqueueAndPlay),
            (Char('g'), KeyModifiers::CONTROL, EnqueueAndPlay),
            (Char('f'), NONE, MoveUp),
            (Char('g'), NONE, MoveDown),
            (Delete, NONE, Remove),
            (Backspace, NONE, ClearQueue),
            // The tab strip is numbered, so those digits switch tabs the way
            // a browser's do.
            (Char('1'), NONE, ShowQueue),
            (Char('2'), NONE, ShowLibrary),
            (Char('3'), NONE, ShowTrack),
            (Char('4'), NONE, ShowSearch),
            // Views (kew uses F2-F6)
            (F(2), NONE, ShowQueue),
            (F(3), NONE, ShowLibrary),
            (F(4), NONE, ShowTrack),
            (F(5), NONE, ShowSearch),
            (F(6), NONE, ShowHelp),
            (Char('m'), NONE, ShowLyrics),
            (Char('/'), NONE, ShowSearch),
            // Exit
            (Char('P'), SHIFT, PlayAll),
            // btop's shape: escape opens the menu, q quits outright.
            (Esc, NONE, ToggleMenu),
            (Char('q'), NONE, Quit),
        ];
        Self { bindings }
    }
}

impl Keymap {
    pub fn resolve(&self, code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        // Shift is implicit in the char for things like 'S', so compare on the
        // modifiers that actually distinguish a binding.
        let significant = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT);
        self.bindings
            .iter()
            .find(|(c, m, _)| {
                *c == code && (*m & (KeyModifiers::CONTROL | KeyModifiers::ALT)) == significant
            })
            .map(|(_, _, a)| *a)
    }

    /// The key(s) bound to an action, for rendering the help view.
    ///
    /// Modifiers are included: without them `ctrl+g` renders as a bare `g`,
    /// which collides with whatever plain `g` is bound to.
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|(_, _, a)| *a == action)
            .map(|(c, m, _)| {
                let mut s = String::new();
                if m.contains(KeyModifiers::CONTROL) {
                    s.push_str("ctrl+");
                }
                if m.contains(KeyModifiers::ALT) {
                    s.push_str("alt+");
                }
                s.push_str(&key_name(*c));
                s
            })
            .collect()
    }
}

fn key_name(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "shift+tab".into(),
        KeyCode::Backspace => "bksp".into(),
        KeyCode::Delete => "del".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdn".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kew_bindings_resolve() {
        let km = Keymap::default();
        assert_eq!(
            km.resolve(KeyCode::Char(' '), KeyModifiers::NONE),
            Some(Action::PlayPause)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('l'), KeyModifiers::NONE),
            Some(Action::Next)
        );
        assert_eq!(
            km.resolve(KeyCode::F(4), KeyModifiers::NONE),
            Some(Action::ShowTrack)
        );
        assert_eq!(km.resolve(KeyCode::Char('~'), KeyModifiers::NONE), None);
    }

    #[test]
    fn help_view_can_find_keys() {
        let km = Keymap::default();
        let keys = km.keys_for(Action::PlayPause);
        assert!(keys.contains(&"space".to_string()));
        assert!(keys.contains(&"p".to_string()));
    }

    #[test]
    fn escape_opens_the_menu_and_q_quits() {
        // btop's convention: escape is the menu, not an exit.
        let km = Keymap::default();
        assert_eq!(
            km.resolve(KeyCode::Esc, KeyModifiers::NONE),
            Some(Action::ToggleMenu)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('q'), KeyModifiers::NONE),
            Some(Action::Quit)
        );
    }

    #[test]
    fn numbered_tabs_match_the_strip() {
        let km = Keymap::default();
        for (ch, action) in [
            ('1', Action::ShowQueue),
            ('2', Action::ShowLibrary),
            ('3', Action::ShowTrack),
            ('4', Action::ShowSearch),
        ] {
            assert_eq!(
                km.resolve(KeyCode::Char(ch), KeyModifiers::NONE),
                Some(action),
                "digit {ch} should select its tab"
            );
        }
    }

    #[test]
    fn help_view_spells_out_modifiers() {
        // ctrl+g must not render as a bare "g", which is Move-down.
        let km = Keymap::default();
        let keys = km.keys_for(Action::EnqueueAndPlay);
        assert!(keys.contains(&"ctrl+g".to_string()), "got {keys:?}");
        assert!(keys.contains(&"alt+enter".to_string()), "got {keys:?}");
        assert!(!keys.contains(&"g".to_string()));
    }

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
        let c: Config =
            toml::from_str("accent_color = 1\ncolor_from_cover = false\n").unwrap();
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
    fn state_round_trips_and_falls_back_to_config_volume() {
        let dir = std::env::temp_dir().join(format!("ytkew-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = Config {
            initial_volume: 42.0,
            ..Config::default()
        };
        // No state file yet -> use the config's initial volume.
        let s = State::load(&dir, &cfg);
        assert_eq!(s.volume, 42.0);

        let saved = State {
            volume: 77.0,
            shuffle: true,
            repeat: "all".into(),
            cover_cell: [12, 24],
            cover_visible: false,
            theme: "nord".into(),
        };
        saved.save(&dir).unwrap();
        let back = State::load(&dir, &cfg);
        assert_eq!(back.volume, 77.0);
        assert!(back.shuffle);
        assert_eq!(back.repeat, "all");
        assert_eq!(back.cover_cell, [12, 24], "tuned cell size must persist");
        assert!(!back.cover_visible, "the b toggle must persist");
        assert_eq!(back.theme, "nord", "the chosen theme must persist");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(after.contains("# my comments"), "user comments must survive");
        assert!(after.contains("accent_color = 9"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
