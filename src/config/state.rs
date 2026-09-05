//! Runtime state the app owns and rewrites.
//!
//! Kept apart from [`super::Config`], which belongs to the user: ytkew only
//! ever reads that file, so hand-edits and comments survive.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::Config;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
}
