//! Installing the desktop entry and icon from inside the binary.
//!
//! `make install` puts these in place, but `cargo install ytkew` cannot: it
//! copies one executable and knows nothing about data files. So the binary
//! carries both and can put them down itself.
//!
//! Doing this from a build script would be the obvious alternative and is the
//! wrong tool -- build scripts are supposed to write only to `OUT_DIR`, they
//! run for every `cargo build` rather than only on install, and under a
//! distro packager they run as a build user with no home directory to install
//! into. Running it when the user asks avoids all three.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Shipped alongside the binary rather than read from disk, since after
/// `cargo install` there is no source tree to read from.
const DESKTOP_ENTRY: &str = include_str!("../ytkew.desktop");
const ICON: &str = include_str!("../assets/ytkew.svg");

/// `$XDG_DATA_HOME`, or the default it stands in for.
fn data_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::home_dir()
        .context("no home directory to install into")?
        .join(".local/share"))
}

fn entry_path(data: &Path) -> PathBuf {
    data.join("applications/ytkew.desktop")
}

fn icon_path(data: &Path) -> PathBuf {
    data.join("icons/hicolor/scalable/apps/ytkew.svg")
}

/// Is the launcher entry already in place?
pub fn is_installed() -> bool {
    data_home().is_ok_and(|d| entry_path(&d).is_file())
}

/// Write the desktop entry and icon, and refresh the caches that notice them.
pub fn install() -> Result<Vec<PathBuf>> {
    let data = data_home()?;
    let mut written = Vec::new();
    for (path, contents) in [(entry_path(&data), DESKTOP_ENTRY), (icon_path(&data), ICON)] {
        let parent = path.parent().context("no parent directory")?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        written.push(path);
    }

    // Best effort: the entry works without these, they just make it show up
    // sooner. Absent on plenty of systems, which is not an error.
    refresh("update-desktop-database", &[data.join("applications")]);
    refresh(
        "gtk-update-icon-cache",
        &["-qtf".into(), data.join("icons/hicolor")],
    );
    Ok(written)
}

/// Remove what `install` wrote.
pub fn uninstall() -> Result<Vec<PathBuf>> {
    let data = data_home()?;
    let mut removed = Vec::new();
    for path in [entry_path(&data), icon_path(&data)] {
        if path.is_file() {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed.push(path);
        }
    }
    refresh("update-desktop-database", &[data.join("applications")]);
    Ok(removed)
}

fn refresh(program: &str, args: &[PathBuf]) {
    let _ = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Point XDG_DATA_HOME somewhere disposable. Serialised, because the
    /// environment is process-global and the tests run in threads.
    fn with_data_home<T>(tag: &str, f: impl FnOnce(&Path) -> T) -> T {
        use std::sync::Mutex;
        static ENV: Mutex<()> = Mutex::new(());
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());

        let dir = std::env::temp_dir().join(format!("ytkew-xdg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let saved = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", &dir);
        let out = f(&dir);
        match saved {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn the_embedded_entry_is_a_valid_desktop_file() {
        // If this drifts, launchers silently ignore the entry.
        assert!(DESKTOP_ENTRY.starts_with("[Desktop Entry]"));
        for key in ["Type=Application", "Name=ytkew", "Exec=ytkew", "Icon=ytkew"] {
            assert!(DESKTOP_ENTRY.contains(key), "missing {key}");
        }
    }

    #[test]
    fn the_embedded_icon_is_an_svg() {
        assert!(ICON.contains("<svg"));
        assert!(ICON.contains("</svg>"));
    }

    #[test]
    fn the_icon_name_matches_the_file_it_is_installed_as() {
        // Icon=ytkew only resolves if the file is named ytkew.svg.
        assert!(DESKTOP_ENTRY.contains("Icon=ytkew"));
        assert_eq!(icon_path(Path::new("/x")).file_name().unwrap(), "ytkew.svg");
    }

    #[test]
    fn install_puts_both_files_where_the_spec_says() {
        with_data_home("install", |dir| {
            assert!(!is_installed());
            let written = install().unwrap();
            assert_eq!(written.len(), 2);
            assert!(dir.join("applications/ytkew.desktop").is_file());
            assert!(dir.join("icons/hicolor/scalable/apps/ytkew.svg").is_file());
            assert!(is_installed());
        });
    }

    #[test]
    fn installing_twice_is_not_an_error() {
        with_data_home("twice", |_| {
            install().unwrap();
            install().unwrap();
            assert!(is_installed());
        });
    }

    #[test]
    fn uninstall_removes_what_install_wrote() {
        with_data_home("uninstall", |dir| {
            install().unwrap();
            let removed = uninstall().unwrap();
            assert_eq!(removed.len(), 2);
            assert!(!is_installed());
            assert!(!dir.join("icons/hicolor/scalable/apps/ytkew.svg").exists());
            // And removing again is a no-op rather than a failure.
            assert!(uninstall().unwrap().is_empty());
        });
    }
}
