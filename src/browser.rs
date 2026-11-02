//! Lifting an existing YouTube Music login out of the browser's cookie store.
//!
//! Copying a Cookie header out of devtools works but is fiddly enough that
//! people get it wrong, so this reads the same cookies straight from disk.
//!
//! Firefox-family discovery keeps the existing SQLite snapshot reader.
//! Explicit browser selection uses rookie, including desktop-keyring support
//! for Chromium browsers. Browser dispatch and cookie filtering are adapted
//! from WakaTaira/ytmusic-tui's auth_cli.rs (MIT); see licenses/ytmusic-tui-MIT.txt.

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
    "SIDCC",
    "__Secure-3PSIDCC",
    "__Secure-1PSIDCC",
];

#[cfg(feature = "browser-cookies")]
const SUPPORTED_BROWSERS: &str = "firefox, librewolf, zen, chrome, chromium, brave, edge, vivaldi, opera, opera_gx, arc, safari (macOS only)";

#[cfg(feature = "browser-cookies")]
type CookieReader = fn(Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>>;

#[cfg(feature = "browser-cookies")]
fn browser_reader(name: &str) -> Result<CookieReader> {
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "firefox" | "ff" => rookie::firefox,
        "librewolf" => rookie::librewolf,
        "zen" => rookie::zen,
        "chrome" | "google-chrome" => rookie::chrome,
        "chromium" | "chromium-browser" => rookie::chromium,
        "brave" | "brave-browser" => rookie::brave,
        "edge" | "msedge" | "microsoft-edge" => rookie::edge,
        "vivaldi" => rookie::vivaldi,
        "opera" => rookie::opera,
        "opera_gx" | "opera-gx" | "operagx" => rookie::opera_gx,
        "arc" => rookie::arc,
        #[cfg(target_os = "macos")]
        "safari" => rookie::safari,
        #[cfg(not(target_os = "macos"))]
        "safari" => bail!("Safari cookie extraction is only supported on macOS"),
        _ => bail!("unsupported browser {name:?}; supported: {SUPPORTED_BROWSERS}"),
    })
}

/// Read the selected browser's YouTube login without printing cookie values.
pub fn extract_from_browser(name: &str) -> Result<String> {
    #[cfg(feature = "browser-cookies")]
    {
        let domains = Some(vec!["youtube.com".to_owned()]);
        let firefox =
            name.trim().eq_ignore_ascii_case("firefox") || name.trim().eq_ignore_ascii_case("ff");
        let result = browser_reader(name)?(domains);
        // Browser errors may include decrypted data. Give recovery steps without
        // forwarding the extractor's raw error or cookie values to the terminal.
        let cookies = match result {
            Ok(cookies) => cookies,
            Err(_) if firefox => return firefox_cookie_fallback(),
            Err(_) => bail!(
                "could not read cookies from {name}; check that the browser is installed, \
             unlock your desktop keyring, or use `ytkew --auth cookie`"
            ),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let header = header_from_cookies(cookies, now);
        if header.is_err() && firefox {
            return firefox_cookie_fallback();
        }
        header
    }
    #[cfg(not(feature = "browser-cookies"))]
    {
        let _ = name;
        bail!("this build has the `browser-cookies` feature disabled -- use `ytkew --auth cookie`")
    }
}

#[cfg(feature = "browser-cookies")]
fn header_from_cookies(cookies: Vec<rookie::enums::Cookie>, now: u64) -> Result<String> {
    let mut pairs = std::collections::BTreeMap::new();
    for cookie in cookies {
        let domain = cookie.domain.trim_start_matches('.');
        if !(domain == "youtube.com" || domain.ends_with(".youtube.com"))
            || !WANTED.contains(&cookie.name.as_str())
            || cookie.value.is_empty()
            || cookie
                .expires
                .is_some_and(|expires| expires != 0 && expires <= now)
        {
            continue;
        }
        if !cookie.value.is_ascii() || cookie.value.contains(['\r', '\n', ';']) {
            bail!("browser returned an invalid cookie value; sign in again and retry");
        }
        pairs.entry(cookie.name).or_insert(cookie.value);
    }
    if !pairs.contains_key("SAPISID") {
        let signing_cookie = pairs
            .get("__Secure-3PAPISID")
            .or_else(|| pairs.get("__Secure-1PAPISID"))
            .cloned()
            .context("no SAPISID cookie -- sign in at music.youtube.com in the selected browser")?;
        pairs.insert("SAPISID".into(), signing_cookie);
    }
    // ytmapi-rs splits SAPISID at the next semicolon, including when it is last.
    Ok(pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value};"))
        .collect::<Vec<_>>()
        .join(" "))
}

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
    let mut roots = firefox_roots(&home).to_vec();
    roots.extend([
        home.join(".librewolf"),
        home.join(".var/app/io.gitlab.librewolf-community/.librewolf"),
        home.join(".waterfox"),
        home.join(".zen"),
    ]);
    scan_profiles(roots)
}

fn firefox_roots(home: &std::path::Path) -> [PathBuf; 5] {
    [
        home.join(".mozilla/firefox"),
        home.join(".config/mozilla/firefox"),
        home.join("snap/firefox/common/.mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        home.join("Library/Application Support/Firefox/Profiles"),
    ]
}

#[cfg(any(feature = "browser-cookies", test))]
fn firefox_profiles_in(home: &std::path::Path) -> Vec<PathBuf> {
    scan_profiles(firefox_roots(home))
}

fn scan_profiles(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
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
    find_cookies_in_profiles(profiles())
}

#[cfg(feature = "browser-cookies")]
fn firefox_cookie_fallback() -> Result<String> {
    let home = dirs::home_dir().context("could not locate the Firefox profile directory")?;
    find_cookies_in_profiles(firefox_profiles_in(&home)).map(|found| found.header)
}

#[cfg(feature = "browser-cookies")]
fn find_cookies_in_profiles(profiles: Vec<PathBuf>) -> Result<Found> {
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

    #[test]
    fn firefox_discovery_finds_modern_profiles_without_searching_other_browsers() {
        let home =
            std::env::temp_dir().join(format!("ytkew-firefox-discovery-{}", std::process::id()));
        let firefox = home.join(".config/mozilla/firefox/default-release");
        let librewolf = home.join(".librewolf/default");
        for profile in [&firefox, &librewolf] {
            std::fs::create_dir_all(profile).unwrap();
            std::fs::write(profile.join("cookies.sqlite"), []).unwrap();
        }
        let found = firefox_profiles_in(&home);
        std::fs::remove_dir_all(home).unwrap();
        assert_eq!(found, vec![firefox]);
    }

    #[cfg(feature = "browser-cookies")]
    #[test]
    fn browser_names_and_aliases_are_recognized() {
        for name in [
            "firefox",
            "librewolf",
            "zen",
            "chrome",
            "chromium",
            "brave",
            "edge",
            "vivaldi",
            "opera",
            "opera_gx",
            "arc",
            " BRAVE ",
            "google-chrome",
            "chromium-browser",
            "msedge",
            "opera-gx",
        ] {
            assert!(browser_reader(name).is_ok(), "{name}");
        }
        assert!(browser_reader("not-a-browser").is_err());
    }

    #[cfg(feature = "browser-cookies")]
    fn cookie(domain: &str, name: &str, value: &str) -> rookie::enums::Cookie {
        rookie::enums::Cookie {
            domain: domain.into(),
            path: "/".into(),
            name: name.into(),
            value: value.into(),
            secure: true,
            expires: None,
            http_only: true,
            same_site: 0,
        }
    }

    #[cfg(feature = "browser-cookies")]
    #[test]
    fn extracted_header_filters_domains_names_empty_and_expired_cookies() {
        let mut expired = cookie(".youtube.com", "SID", "expired");
        expired.expires = Some(1);
        let header = header_from_cookies(
            vec![
                cookie("evilyoutube.com", "SAPISID", "wrong-domain"),
                cookie(".youtube.com", "SAPISID", "good"),
                cookie("music.youtube.com", "SID", "session"),
                cookie(".youtube.com", "SAPISID", "duplicate"),
                cookie(".youtube.com", "NID", "unrelated"),
                cookie(".youtube.com", "PREF", ""),
                expired,
            ],
            100,
        )
        .unwrap();
        assert_eq!(header, "SAPISID=good; SID=session;");
    }

    #[cfg(feature = "browser-cookies")]
    #[test]
    fn secure_sapisid_can_be_used_when_plain_cookie_is_missing() {
        let header = header_from_cookies(
            vec![cookie(".youtube.com", "__Secure-3PAPISID", "signed-in")],
            100,
        )
        .unwrap();
        assert!(header.contains("SAPISID=signed-in;"));
        assert!(header.ends_with(';'));
    }

    #[cfg(feature = "browser-cookies")]
    #[test]
    fn missing_sign_in_and_malformed_values_are_rejected() {
        assert!(header_from_cookies(vec![cookie(".youtube.com", "SID", "only-sid")], 100).is_err());
        assert!(
            header_from_cookies(vec![cookie(".youtube.com", "SAPISID", "bad\r\nvalue")], 100)
                .is_err()
        );
    }

    #[cfg(all(feature = "browser-cookies", not(target_os = "macos")))]
    #[test]
    fn safari_reports_its_platform_requirement() {
        assert!(extract_from_browser("safari")
            .unwrap_err()
            .to_string()
            .contains("macOS"));
    }

    #[cfg(not(feature = "browser-cookies"))]
    #[test]
    fn browser_extraction_reports_disabled_feature() {
        let err = extract_from_browser("brave").unwrap_err();
        assert!(err.to_string().contains("browser-cookies"));
    }
}
