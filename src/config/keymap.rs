//! Actions and the key table that produces them.
//!
//! Defaults follow kew's bindings, so muscle memory carries over.

use crossterm::event::{KeyCode, KeyModifiers};

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
