//! The escape overlay: the top-level menu and the options pane behind it.

use crate::config::Action;
use crate::queue::RepeatMode;
use crate::ui::View;

use super::*;

/// Entries in the overlay menu, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Options,
    Help,
    Quit,
}

/// What a keypress did while the menu was open.
pub(crate) enum MenuOutcome {
    /// Handled entirely by the menu.
    Consumed,
    /// Not a menu key; the menu closed and normal handling should proceed.
    Fallthrough,
}

pub const MENU_ITEMS: [MenuItem; 3] = [MenuItem::Options, MenuItem::Help, MenuItem::Quit];

/// Which pane of the overlay is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScreen {
    Main,
    Options,
}

/// A row in the options pane. Each is a fixed list of choices stepped through
/// with the left and right arrows, which is how btop's options read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Theme,
    Keys,
    SidePane,
    LibraryLayout,
    Renderer,
    ShowCover,
    Visualizer,
    Shuffle,
    Repeat,
    AutoplayRadio,
}

pub const SETTINGS: [Setting; 10] = [
    Setting::Theme,
    Setting::Keys,
    Setting::SidePane,
    Setting::LibraryLayout,
    Setting::Renderer,
    Setting::ShowCover,
    Setting::Visualizer,
    Setting::Shuffle,
    Setting::Repeat,
    Setting::AutoplayRadio,
];

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Setting::Theme => "Theme",
            Setting::Keys => "Key bindings",
            Setting::SidePane => "Side pane",
            Setting::LibraryLayout => "Library layout",
            Setting::Renderer => "Cover renderer",
            Setting::ShowCover => "Show cover art",
            Setting::Visualizer => "Visualizer",
            Setting::Shuffle => "Shuffle",
            Setting::Repeat => "Repeat",
            Setting::AutoplayRadio => "Autoplay radio",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Setting::Theme => {
                "Colours for borders, text and accents. \"cover\" samples them from the album art."
            }
            Setting::Keys => {
                "kew keeps kew's keys. vim swaps in vim motions: gg, G, ctrl+d/u, dd, x, J/K to reorder, H/L for previous and next track."
            }
            Setting::SidePane => {
                "What sits beside the now-playing column on a wide terminal. j/k and enter act on it."
            }
            Setting::LibraryLayout => {
                "Columns puts each level side by side, file-manager style; left and right walk in and out. Narrow panes fall back to the tree."
            }
            Setting::Renderer => {
                "How album art is drawn. Kitty sizes in cells and needs no tuning; sixel needs an accurate cell size; blocks work anywhere."
            }
            Setting::ShowCover => "Show the artwork at all. Same as pressing b.",
            Setting::Visualizer => {
                "Spectrum style. Braille packs four levels per cell; off reclaims the rows."
            }
            Setting::Shuffle => "Play the queue in a random order.",
            Setting::Repeat => "Off stops at the end, all wraps, one loops the track.",
            Setting::AutoplayRadio => {
                "After playing a single search hit, append YouTube's radio mix so it keeps going."
            }
        }
    }
}

impl App {
    /// The word rendered in block letters for a top-level menu row.
    pub fn menu_word(item: MenuItem) -> &'static str {
        match item {
            MenuItem::Options => "OPTIONS",
            MenuItem::Help => "HELP",
            MenuItem::Quit => "QUIT",
        }
    }

    /// The choices for a setting and which one is current.
    pub fn setting_choices(&self, s: Setting) -> (Vec<String>, usize) {
        let pick = |names: &[&str], cur: &str| -> (Vec<String>, usize) {
            let v: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            let i = v
                .iter()
                .position(|n| n.eq_ignore_ascii_case(cur))
                .unwrap_or(0);
            (v, i)
        };
        match s {
            Setting::Theme => {
                let names = self.themes.names();
                let i = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&self.theme))
                    .unwrap_or(0);
                (names, i)
            }
            Setting::Keys => pick(&["kew", "vim"], self.cfg.keys.name()),
            Setting::SidePane => pick(&["off", "queue", "library"], self.cfg.side_pane.name()),
            Setting::LibraryLayout => pick(&["columns", "tree"], self.cfg.library_layout.name()),
            Setting::Renderer => pick(
                &["auto", "kitty", "sixel", "blocks"],
                self.cfg.cover_mode.name(),
            ),
            Setting::ShowCover => pick(
                &["on", "off"],
                if self.cover_visible { "on" } else { "off" },
            ),
            Setting::Visualizer => pick(
                &["bars", "braille", "off"],
                match self.cfg.visualizer_mode {
                    crate::config::VisualizerMode::Bars => "bars",
                    crate::config::VisualizerMode::Braille => "braille",
                    crate::config::VisualizerMode::Off => "off",
                },
            ),
            Setting::Shuffle => pick(
                &["off", "on"],
                if self.queue.shuffle { "on" } else { "off" },
            ),
            Setting::Repeat => pick(
                &["off", "all", "one"],
                match self.queue.repeat {
                    RepeatMode::Off => "off",
                    RepeatMode::All => "all",
                    RepeatMode::One => "one",
                },
            ),
            Setting::AutoplayRadio => pick(
                &["on", "off"],
                if self.cfg.autoplay_radio { "on" } else { "off" },
            ),
        }
    }

    /// Current value of a setting, for display.
    pub fn setting_value(&self, s: Setting) -> String {
        let (choices, i) = self.setting_choices(s);
        choices.get(i).cloned().unwrap_or_default()
    }

    /// Step a setting left or right, wrapping.
    pub fn adjust_setting(&mut self, s: Setting, delta: i32) {
        let (choices, i) = self.setting_choices(s);
        if choices.is_empty() {
            return;
        }
        let n = choices.len() as i32;
        let next = (((i as i32 + delta) % n) + n) % n;
        let value = choices[next as usize].clone();
        self.apply_setting(s, &value);
    }

    pub(crate) fn apply_setting(&mut self, s: Setting, value: &str) {
        use crate::config::{CoverMode, VisualizerMode};
        match s {
            Setting::Theme => {
                self.theme = value.to_string();
                self.apply_theme();
            }
            Setting::Keys => {
                self.cfg.keys = crate::config::keymap::KeyPreset::from_name(value);
                self.keymap = crate::config::Keymap::for_preset(self.cfg.keys);
                // A half-typed sequence makes no sense under the new table.
                self.pending_key = None;
            }
            Setting::SidePane => {
                // The player column changes width, so the art must be redrawn.
                self.clear_cover_art();
                self.cfg.side_pane = crate::config::SidePane::from_name(value);
            }
            Setting::LibraryLayout => {
                self.cfg.library_layout = crate::config::LibraryLayout::from_name(value);
            }
            Setting::Renderer => {
                self.clear_cover_art();
                self.cfg.cover_mode = match value {
                    "kitty" => CoverMode::Kitty,
                    "sixel" => CoverMode::Sixel,
                    "blocks" => CoverMode::Blocks,
                    _ => CoverMode::Auto,
                };
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
            Setting::ShowCover => {
                self.clear_cover_art();
                self.cover_visible = value == "on";
            }
            Setting::Visualizer => {
                self.cfg.visualizer_mode = match value {
                    "braille" => VisualizerMode::Braille,
                    "off" => VisualizerMode::Off,
                    _ => VisualizerMode::Bars,
                };
                // The visualizer changes the layout, so any art must be redrawn.
                self.clear_cover_art();
            }
            Setting::Shuffle => {
                if (value == "on") != self.queue.shuffle {
                    self.queue.toggle_shuffle();
                    self.resync_prefetch();
                }
            }
            Setting::Repeat => {
                self.queue.repeat = match value {
                    "all" => RepeatMode::All,
                    "one" => RepeatMode::One,
                    _ => RepeatMode::Off,
                };
                let _ = self
                    .player
                    .set_loop_file(self.queue.repeat == RepeatMode::One);
                self.resync_prefetch();
            }
            Setting::AutoplayRadio => self.cfg.autoplay_radio = value == "on",
        }
    }

    /// Keys while the overlay is open.
    pub(crate) fn handle_menu_action(&mut self, action: Action) -> MenuOutcome {
        if !self.menu_open {
            return MenuOutcome::Fallthrough;
        }
        match self.menu_screen {
            MenuScreen::Main => match action {
                Action::ScrollUp => self.menu_sel = self.menu_sel.saturating_sub(1),
                Action::ScrollDown => self.menu_sel = (self.menu_sel + 1).min(MENU_ITEMS.len() - 1),
                Action::ToggleMenu => self.menu_open = false,
                Action::Enqueue | Action::EnqueueAndPlay => {
                    match MENU_ITEMS[self.menu_sel.min(MENU_ITEMS.len() - 1)] {
                        MenuItem::Options => {
                            self.menu_screen = MenuScreen::Options;
                            self.option_sel = 0;
                        }
                        MenuItem::Help => {
                            self.menu_open = false;
                            self.set_view(View::Help);
                        }
                        MenuItem::Quit => self.should_quit = true,
                    }
                }
                Action::Quit => self.should_quit = true,
                _ => {
                    self.menu_open = false;
                    return MenuOutcome::Fallthrough;
                }
            },
            MenuScreen::Options => match action {
                Action::ScrollUp => self.option_sel = self.option_sel.saturating_sub(1),
                Action::ScrollDown => {
                    self.option_sel = (self.option_sel + 1).min(SETTINGS.len() - 1)
                }
                // Left and right step the value, which is what the arrows
                // beside the selected row advertise.
                Action::Prev => {
                    let s = SETTINGS[self.option_sel.min(SETTINGS.len() - 1)];
                    self.adjust_setting(s, -1);
                }
                Action::Next | Action::Enqueue | Action::EnqueueAndPlay => {
                    let s = SETTINGS[self.option_sel.min(SETTINGS.len() - 1)];
                    self.adjust_setting(s, 1);
                }
                // Escape backs out to the menu rather than closing outright.
                Action::ToggleMenu => self.menu_screen = MenuScreen::Main,
                Action::Quit => self.should_quit = true,
                _ => {}
            },
        }
        MenuOutcome::Consumed
    }

    /// Play everything the current view implies: the selected container in
    /// the library, all search results, or the queue from its start.
    pub fn play_all_in_context(&mut self) {
        match self.view {
            View::Search => {
                let all = self.search_results.clone();
                if all.is_empty() {
                    self.notify("nothing to play");
                } else {
                    self.play_all(all, 0);
                }
            }
            View::Library => self.play_selected_library_node(),
            _ => {
                if self.queue.is_empty() {
                    self.notify("queue is empty");
                } else {
                    self.queue.jump_to(0);
                    self.start_current();
                }
            }
        }
    }
}
