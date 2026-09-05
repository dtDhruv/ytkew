//! Lifting an existing YouTube Music login out of the browser's cookie store.
//!
//! Copying a Cookie header out of devtools works but is fiddly enough that
//! people get it wrong, so this reads the same cookies straight from disk.
//!
//! Firefox only. Its store is plain SQLite, so this is a query. Chromium
//! encrypts cookie values with a key held in the desktop keyring, which needs
//! both a keyring client and AES-GCM to undo -- worth doing, but not the same
//! job. `ytkew --auth cookie` still takes a pasted header from any browser.

use anyhow::{bail, Result};
use std::path::PathBuf;

#[cfg(feature = "browser-cookies")]
use anyhow::{anyhow, Context};
#[cfg(feature = "browser-cookies")]
use std::path::Path;

#[cfg(feature = "browser-cookies")]
/// Cookies YouTube Music authenticates with. `SAPISID` is the one that
/// actually signs requests; the rest come along because the endpoint expects
/// a browser's full jar and behaves oddly without them.
const WANTED: &[&str] = &[
    "SAPISID",
    "__Secure-1PAPISID",
    "__Secure-3PAPISID",
    "SID",
    "__Secure-1PSID",
    "__Secure-3PSID",
    "HSID",
    "SSID",
    "APISID",
    "LOGIN_INFO",
    "PREF",
    "VISITOR_INFO1_LIVE",
    "YSC",
];

/// A profile that yielded a usable cookie header.
pub struct Found {
    pub profile: PathBuf,
    pub header: String,
    /// Which of the wanted cookies were actually present.
    pub names: Vec<String>,
}

/// Every Firefox-family profile directory that has a cookie store.
///
/// Covers the packaging layouts a Linux user is likely to have -- native,
/// snap and flatpak -- plus the forks that keep Firefox's on-disk shape.
pub fn profiles() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let roots = [
        home.join(".mozilla/firefox"),
        home.join("snap/firefox/common/.mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        home.join(".librewolf"),
        home.join(".var/app/io.gitlab.librewolf-community/.librewolf"),
        home.join(".waterfox"),
        home.join(".zen"),
        // macOS, for anyone building there.
        home.join("Library/Application Support/Firefox/Profiles"),
    ];
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if dir.join("cookies.sqlite").is_file() {
                found.push(dir);
            }
        }
    }
    // Most recently written first: that is the profile in active use.
    found.sort_by_key(|p| {
        std::fs::metadata(p.join("cookies.sqlite"))
            .and_then(|m| m.modified())
            .ok()
    });
    found.reverse();
    found
}

/// Read YouTube cookies from every profile and return the first usable set.
#[cfg(feature = "browser-cookies")]
pub fn find_cookies() -> Result<Found> {
    let profiles = profiles();
    if profiles.is_empty() {
        bail!("no Firefox profile found -- use `ytkew --auth cookie` to paste a header instead");
    }
    let mut last_err = None;
    for profile in profiles {
        match read_profile(&profile) {
            Ok(found) => return Ok(found),
            Err(e) => last_err = Some(format!("{}: {e}", profile.display())),
        }
    }
    Err(anyhow!(
        "no signed-in YouTube Music session in any Firefox profile ({})",
        last_err.unwrap_or_else(|| "no detail".into())
    ))
}

#[cfg(not(feature = "browser-cookies"))]
pub fn find_cookies() -> Result<Found> {
    bail!("this build has the `browser-cookies` feature disabled -- use `ytkew --auth cookie`")
}

#[cfg(feature = "browser-cookies")]
fn read_profile(profile: &Path) -> Result<Found> {
    // Firefox keeps the store open in WAL mode, so read a snapshot rather
    // than the live file: opening it directly can block or see a torn view,
    // and the -wal side file holds writes not yet folded into the main one.
    let tmp = tempdir_for(profile)?;
    let db = tmp.join("cookies.sqlite");
    std::fs::copy(profile.join("cookies.sqlite"), &db).context("copying the cookie store")?;
    for side in ["cookies.sqlite-wal", "cookies.sqlite-shm"] {
        let from = profile.join(side);
        if from.is_file() {
            let _ = std::fs::copy(&from, tmp.join(side));
        }
    }
    let result = query(&db);
    // The snapshot is a live credential; do not leave it lying in /tmp.
    let _ = std::fs::remove_dir_all(&tmp);
    result.map(|(header, names)| Found {
        profile: profile.to_path_buf(),
        header,
        names,
    })
}

#[cfg(feature = "browser-cookies")]
fn query(db: &Path) -> Result<(String, Vec<String>)> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .context("opening the cookie store")?;
    let mut stmt = conn
        .prepare("SELECT name, value FROM moz_cookies WHERE host LIKE '%youtube.com'")
        .context("querying cookies")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .context("reading cookies")?;

    let mut pairs: Vec<(String, String)> = Vec::new();
    for row in rows.flatten() {
        if WANTED.contains(&row.0.as_str()) && !row.1.is_empty() {
            pairs.push(row);
        }
    }
    if !pairs.iter().any(|(n, _)| n == "SAPISID") {
        bail!("no SAPISID cookie -- not signed in to YouTube in this profile");
    }
    // Stable order so the file does not churn between runs.
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.dedup_by(|a, b| a.0 == b.0);

    let names = pairs.iter().map(|(n, _)| n.clone()).collect();
    // The trailing semicolon is not cosmetic: ytmapi-rs finds SAPISID by
    // splitting on the ';' after it, so a header ending on that cookie would
    // fail to parse.
    let header = pairs
        .iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
        + ";";
    Ok((header, names))
}

#[cfg(feature = "browser-cookies")]
fn tempdir_for(profile: &Path) -> Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    profile.hash(&mut h);
    let dir = std::env::temp_dir().join(format!(
        "ytkew-cookies-{}-{:x}",
        std::process::id(),
        h.finish()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).context("creating a scratch directory")?;
    // Owner-only: the snapshot inside is a live credential.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "browser-cookies")]
    #[test]
    fn only_auth_cookies_are_collected() {
        // Guards against widening the set by accident: everything here is
        // sent to Google on every request, so the list should stay minimal
        // and deliberate.
        assert!(WANTED.contains(&"SAPISID"));
        assert!(!WANTED.contains(&"NID"), "NID is an ads cookie, not auth");
        let mut sorted = WANTED.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before, "duplicate entry in WANTED");
    }

    #[test]
    fn profile_discovery_never_panics_without_a_browser() {
        // Returns empty rather than erroring when no browser is installed.
        let _ = profiles();
    }
}
