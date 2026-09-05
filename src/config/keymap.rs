//! Actions and the key table that produces them.
//!
//! Two presets. `kew` follows kew's bindings so muscle memory carries over,
//! and is the default. `vim` keeps the playback keys but replaces navigation
//! with the motions a vim user already has in their fingers, including the
//! two-key `gg` and `dd`.

use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};

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

/// Which set of bindings to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyPreset {
    /// kew's bindings, so muscle memory carries over.
    #[default]
    Kew,
    /// vim motions for navigation, kew's keys for playback.
    Vim,
}

impl KeyPreset {
    pub fn name(self) -> &'static str {
        match self {
            KeyPreset::Kew => "kew",
            KeyPreset::Vim => "vim",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "vim" => KeyPreset::Vim,
            _ => KeyPreset::Kew,
        }
    }
}

/// A resolved key -> action table.
pub struct Keymap {
    bindings: Vec<(KeyCode, KeyModifiers, Action)>,
    /// Two-key sequences, `gg` style. Held separately because the first key
    /// has to be recognised as a prefix before the second arrives.
    sequences: Vec<(char, char, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::for_preset(KeyPreset::Kew)
    }
}

impl Keymap {
    pub fn for_preset(preset: KeyPreset) -> Self {
        match preset {
            KeyPreset::Kew => Self::kew(),
            KeyPreset::Vim => Self::vim(),
        }
    }

    /// kew's bindings, verbatim where they make sense for a streaming client.
    fn kew() -> Self {
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
        Self {
            bindings,
            sequences: Vec::new(),
        }
    }

    /// vim motions for moving around, kew's keys for the transport.
    ///
    /// Deliberately not a full modal editor: there is no insert mode to leave
    /// and no buffer to edit, so the useful part is the motions.
    fn vim() -> Self {
        use Action::*;
        use KeyCode::*;
        const NONE: KeyModifiers = KeyModifiers::NONE;
        const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
        const CTRL: KeyModifiers = KeyModifiers::CONTROL;

        let bindings = vec![
            // Playback. h/l are motions in vim, so the transport moves to
            // shifted keys and the arrows.
            (Char(' '), NONE, PlayPause),
            (Char('p'), NONE, PlayPause),
            (Char('S'), SHIFT, Stop),
            (Char('L'), SHIFT, Next),
            (Char('H'), SHIFT, Prev),
            (Right, NONE, Next),
            (Left, NONE, Prev),
            // In the library's column view these ascend and descend; the
            // transport keeps H/L and the arrows everywhere else.
            (Char('l'), NONE, Next),
            (Char('h'), NONE, Prev),
            (Char('w'), NONE, SeekForward),
            (Char('b'), NONE, SeekBack),
            // Volume
            (Char('+'), NONE, VolumeUp),
            (Char('='), NONE, VolumeUp),
            (Char('-'), NONE, VolumeDown),
            // Motions
            (Char('j'), NONE, ScrollDown),
            (Char('k'), NONE, ScrollUp),
            (Down, NONE, ScrollDown),
            (Up, NONE, ScrollUp),
            (Char('d'), CTRL, Action::PageDown),
            (Char('u'), CTRL, Action::PageUp),
            (Char('f'), CTRL, Action::PageDown),
            (Char('b'), CTRL, Action::PageUp),
            (Char('G'), SHIFT, Bottom),
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
            (Char('c'), NONE, ToggleAscii),
            (Char('.'), NONE, ToggleLike),
            (Char('R'), SHIFT, StartRadio),
            (Char('['), NONE, CoverSmaller),
            (Char(']'), NONE, CoverBigger),
            // Queue editing. x deletes under the cursor, dd does the same
            // for anyone whose fingers reach for it first.
            (Enter, NONE, Enqueue),
            (Enter, KeyModifiers::ALT, EnqueueAndPlay),
            (Char('o'), NONE, EnqueueAndPlay),
            (Char('J'), SHIFT, MoveDown),
            (Char('K'), SHIFT, MoveUp),
            (Char('x'), NONE, Remove),
            (Delete, NONE, Remove),
            (Backspace, NONE, ClearQueue),
            // Views
            (Char('1'), NONE, ShowQueue),
            (Char('2'), NONE, ShowLibrary),
            (Char('3'), NONE, ShowTrack),
            (Char('4'), NONE, ShowSearch),
            (F(2), NONE, ShowQueue),
            (F(3), NONE, ShowLibrary),
            (F(4), NONE, ShowTrack),
            (F(5), NONE, ShowSearch),
            (F(6), NONE, ShowHelp),
            (Char('m'), NONE, ShowLyrics),
            (Char('/'), NONE, ShowSearch),
            (Char('P'), SHIFT, PlayAll),
            (Esc, NONE, ToggleMenu),
            (Char('q'), NONE, Quit),
        ];
        let sequences = vec![('g', 'g', Top), ('d', 'd', Remove)];
        Self {
            bindings,
            sequences,
        }
    }

    /// Does this key open a two-key sequence? Such a key produces no action
    /// of its own; the next keypress decides.
    pub fn is_sequence_prefix(&self, code: KeyCode, mods: KeyModifiers) -> Option<char> {
        if mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        match code {
            KeyCode::Char(c) if self.sequences.iter().any(|(a, _, _)| *a == c) => Some(c),
            _ => None,
        }
    }

    /// Complete a sequence opened by `first`.
    pub fn resolve_sequence(&self, first: char, code: KeyCode) -> Option<Action> {
        let KeyCode::Char(second) = code else {
            return None;
        };
        self.sequences
            .iter()
            .find(|(a, b, _)| *a == first && *b == second)
            .map(|(_, _, action)| *action)
    }

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
        let seqs = self
            .sequences
            .iter()
            .filter(|(_, _, a)| *a == action)
            .map(|(x, y, _)| format!("{x}{y}"));
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
            .chain(seqs)
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
    fn vim_motions_resolve() {
        let km = Keymap::for_preset(KeyPreset::Vim);
        assert_eq!(
            km.resolve(KeyCode::Char('j'), KeyModifiers::NONE),
            Some(Action::ScrollDown)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Action::Bottom)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Action::PageDown)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(Action::PageUp)
        );
        // h and l are motions in vim, so the transport moved to H and L.
        assert_eq!(
            km.resolve(KeyCode::Char('L'), KeyModifiers::SHIFT),
            Some(Action::Next)
        );
        // Lowercase h and l carry the same actions, which the library's
        // column view reinterprets as stepping out and in. Outside that pane
        // they are the transport, same as the arrows.
        assert_eq!(
            km.resolve(KeyCode::Char('l'), KeyModifiers::NONE),
            Some(Action::Next)
        );
        assert_eq!(
            km.resolve(KeyCode::Char('h'), KeyModifiers::NONE),
            Some(Action::Prev)
        );
    }

    #[test]
    fn gg_and_dd_need_both_keys() {
        let km = Keymap::for_preset(KeyPreset::Vim);
        // `g` alone commits to nothing; it opens a sequence.
        assert_eq!(km.resolve(KeyCode::Char('g'), KeyModifiers::NONE), None);
        assert_eq!(
            km.is_sequence_prefix(KeyCode::Char('g'), KeyModifiers::NONE),
            Some('g')
        );
        assert_eq!(
            km.resolve_sequence('g', KeyCode::Char('g')),
            Some(Action::Top)
        );
        assert_eq!(
            km.resolve_sequence('d', KeyCode::Char('d')),
            Some(Action::Remove)
        );
        // A mismatched second key is not an action, so the caller falls back.
        assert_eq!(km.resolve_sequence('g', KeyCode::Char('j')), None);
        // ctrl+d is its own binding, never the start of `dd`.
        assert_eq!(
            km.is_sequence_prefix(KeyCode::Char('d'), KeyModifiers::CONTROL),
            None
        );
    }

    #[test]
    fn the_kew_preset_has_no_sequences_so_g_still_acts_alone() {
        let km = Keymap::for_preset(KeyPreset::Kew);
        assert_eq!(
            km.is_sequence_prefix(KeyCode::Char('g'), KeyModifiers::NONE),
            None
        );
        assert_eq!(
            km.resolve(KeyCode::Char('g'), KeyModifiers::NONE),
            Some(Action::MoveDown)
        );
    }

    #[test]
    fn the_help_view_lists_two_key_sequences() {
        let km = Keymap::for_preset(KeyPreset::Vim);
        assert!(km.keys_for(Action::Top).contains(&"gg".to_string()));
    }

    #[test]
    fn both_presets_bind_the_essentials() {
        // Whichever preset is active, the app has to remain usable.
        for preset in [KeyPreset::Kew, KeyPreset::Vim] {
            let km = Keymap::for_preset(preset);
            for action in [
                Action::PlayPause,
                Action::Quit,
                Action::ToggleMenu,
                Action::ScrollUp,
                Action::ScrollDown,
                Action::Top,
                Action::Bottom,
                Action::Next,
                Action::Prev,
                Action::Enqueue,
                Action::ShowSearch,
                Action::ShowLibrary,
            ] {
                assert!(
                    !km.keys_for(action).is_empty(),
                    "{preset:?} has no key for {action:?}"
                );
            }
        }
    }

    #[test]
    fn preset_names_round_trip() {
        for p in [KeyPreset::Kew, KeyPreset::Vim] {
            assert_eq!(KeyPreset::from_name(p.name()), p);
        }
        assert_eq!(KeyPreset::from_name("VIM"), KeyPreset::Vim);
        // An unknown preset falls back rather than leaving the app keyless.
        assert_eq!(KeyPreset::from_name("nonsense"), KeyPreset::Kew);
    }

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
    fn help_view_spells_out_modifiers() {
        // ctrl+g must not render as a bare "g", which is Move-down.
        let km = Keymap::default();
        let keys = km.keys_for(Action::EnqueueAndPlay);
        assert!(keys.contains(&"ctrl+g".to_string()), "got {keys:?}");
        assert!(keys.contains(&"alt+enter".to_string()), "got {keys:?}");
        assert!(!keys.contains(&"g".to_string()));
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
}
