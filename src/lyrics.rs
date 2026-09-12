//! Lyrics: parsing, lookup, and the fallback chain.
//!
//! Pure data plus one HTTP client. No ratatui here on purpose -- the overlay
//! reads [`Lyrics`] and renders; this module never touches the screen.
//!
//! Lookup order (Phase B wires the last step):
//! LRCLIB synced > LRCLIB plain > `ytmapi-rs` > "no lyrics found".
//!
//! Phase B entry points: [`fetch_lyrics`] for the whole chain, or
//! [`fetch_from_lrclib`] plus [`Lyrics::plain`] when the caller wants to own
//! the fallback itself.

/// One line of lyrics. `ms` is milliseconds since track start; plain
/// (unsynced) lines all sit at 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub ms: u32,
    pub text: String,
}

/// Owned lyrics for one track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub is_synced: bool,
    /// "lrclib/synced" | "lrclib/plain" | "ytmapi" | "none".
    pub source: String,
    /// Line texts joined with `\n`, so plain consumers never need to care
    /// whether timestamps existed.
    pub raw: String,
}

impl Lyrics {
    /// Untimed text: one line per row, every `ms` set to 0.
    pub fn plain(text: String, source: &str) -> Self {
        let lines = text
            .lines()
            .map(|l| LyricLine {
                ms: 0,
                text: l.to_string(),
            })
            .collect::<Vec<_>>();
        Self {
            raw: lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            lines,
            is_synced: false,
            source: source.to_string(),
        }
    }

    /// Parse LRC text. Untimed lines (including `[ti:]`/`[length:]` metadata)
    /// are dropped; only timed lines survive. Empty text after a timestamp is
    /// kept -- an empty timed line still advances the highlight over an
    /// instrumental gap. Lines are sorted so [`Lyrics::line_for`] can binary
    /// search; LRC files are usually sorted already, so this is nearly free.
    pub fn parse_lrc(s: &str) -> Self {
        let mut lines = Vec::new();
        for raw_line in s.lines() {
            let mut rest = raw_line.trim_start();
            let mut stamps = Vec::new();
            while let Some((ms, after)) = split_leading_timestamp(rest) {
                stamps.push(ms);
                rest = after;
            }
            if stamps.is_empty() {
                continue;
            }
            // One line carrying several timestamps (a repeated chorus) fans
            // out into one entry per stamp; the stable sort below keeps them
            // in file order for equal timestamps.
            let text = rest.trim().to_string();
            for ms in stamps {
                lines.push(LyricLine {
                    ms,
                    text: text.clone(),
                });
            }
        }
        lines.sort_by_key(|l| l.ms);
        // Synced iff at least one timed line parsed -- even one with empty
        // text, which still marks an instrumental gap for the highlight.
        let is_synced = !lines.is_empty();
        let raw = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            lines,
            is_synced,
            source: "lrclib/synced".to_string(),
            raw,
        }
    }

    /// Index of the last line with `ms <= pos_ms`: the line to highlight.
    /// `None` when there are no lines or playback hasn't reached the first.
    pub fn line_for(&self, pos_ms: u32) -> Option<usize> {
        let n = self.lines.partition_point(|l| l.ms <= pos_ms);
        if n == 0 {
            None
        } else {
            Some(n - 1)
        }
    }
}

/// Parse one leading `[mm:ss.xx]`/`[mm:ss.xxx]`/`[mm:ss]` tag, returning the
/// timestamp in milliseconds and everything after the closing bracket.
/// Anything else -- metadata like `[ti:title]`, garbage, a missing bracket --
/// is `None`. Only the first tag is consumed; [`Lyrics::parse_lrc`] loops this
/// to expand multi-stamp lines.
pub fn parse_lrc_line(s: &str) -> Option<(u32, String)> {
    let (ms, rest) = split_leading_timestamp(s.trim_start())?;
    Some((ms, rest.trim().to_string()))
}

/// Split one leading `[tag]` off `s`. The tag must be a timestamp
/// (`mm:ss` with an optional `.f`/`.ff`/`.fff` fraction); metadata tags fail
/// the numeric parse and fall out as `None` with no special-casing.
fn split_leading_timestamp(s: &str) -> Option<(u32, &str)> {
    let rest = s.strip_prefix('[')?;
    let end = rest.find(']')?;
    let ms = parse_timestamp(&rest[..end])?;
    Some((ms, &rest[end + 1..]))
}

fn parse_timestamp(tag: &str) -> Option<u32> {
    let (mm, rest) = tag.split_once(':')?;
    let minutes: u64 = mm.trim().parse().ok()?;
    let (ss, frac) = match rest.split_once('.') {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    if ss.is_empty() || !ss.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: u64 = ss.trim().parse().ok()?;
    let frac_ms: u64 = if frac.is_empty() {
        0
    } else {
        if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let v: u64 = frac.parse().ok()?;
        v * 10u64.pow(3 - frac.len() as u32)
    };
    // Clamp absurd values instead of overflowing: a garbage timestamp that
    // parses should land at the end, not wrap to the start.
    let total = minutes
        .saturating_mul(60_000)
        .saturating_add(seconds.saturating_mul(1000))
        .saturating_add(frac_ms);
    Some(total.min(u64::from(u32::MAX)) as u32)
}

// --- LRCLIB fetch ----------------------------------------------------------

const LRCLIB_CACHED: &str = "https://lrclib.net/api/get-cached";
const LRCLIB_GET: &str = "https://lrclib.net/api/get";

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

fn lrclib_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent(format!(
            "ytkew/{} (https://github.com/dtDhruv/ytkew)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .unwrap_or_default()
}

/// One LRCLIB endpoint call. `None` means "try the next source", whatever the
/// cause -- network error, 404 (unknown track), 429 (rate-limited; v1 just
/// gives up and falls through rather than honouring Retry-After), or a body
/// that doesn't decode. Collapsing them keeps the fallback chain to one
/// `Option` with no error type Phase B must match on.
async fn query_lrclib(
    client: &reqwest::Client,
    endpoint: &str,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_secs: u64,
) -> Option<LrclibResponse> {
    let duration = duration_secs.to_string();
    let mut req = client
        .get(endpoint)
        .query(&[("track_name", title), ("artist_name", artist)]);
    if let Some(album) = album.filter(|a| !a.is_empty()) {
        req = req.query(&[("album_name", album)]);
    }
    if duration_secs > 0 {
        req = req.query(&[("duration", duration.as_str())]);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // `.text()` + `serde_json` rather than `.json()`: the crate is built with
    // `default-features = false`, so the `json` response helper isn't there.
    let body = resp.text().await.ok()?;
    serde_json::from_str(&body).ok()
}

fn from_lrclib(res: LrclibResponse) -> Option<Lyrics> {
    if let Some(synced) = res.synced_lyrics.filter(|s| !s.trim().is_empty()) {
        let lyrics = Lyrics::parse_lrc(&synced);
        if lyrics.is_synced {
            return Some(lyrics);
        }
    }
    if let Some(plain) = res.plain_lyrics.filter(|s| !s.trim().is_empty()) {
        return Some(Lyrics::plain(plain, "lrclib/plain"));
    }
    if res.instrumental {
        return Some(Lyrics::plain("instrumental".to_string(), "lrclib/plain"));
    }
    None
}

/// LRCLIB only: exact-match endpoint first, search endpoint second.
/// `album` is `None`/empty-safe, `duration_secs` of 0 is simply not sent.
pub async fn fetch_from_lrclib(
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_secs: u64,
) -> Option<Lyrics> {
    // ponytail: no disk cache, add when LRCLIB latency shows.
    let client = lrclib_client();
    for endpoint in [LRCLIB_CACHED, LRCLIB_GET] {
        if let Some(res) =
            query_lrclib(&client, endpoint, title, artist, album, duration_secs).await
        {
            if let Some(lyrics) = from_lrclib(res) {
                return Some(lyrics);
            }
        }
    }
    None
}

/// Full chain: LRCLIB synced > LRCLIB plain > `ytm_fallback` (Phase B passes
/// `api.lyrics(&video_id)` mapped to `Option`) > "no lyrics found".
pub async fn fetch_lyrics(
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_secs: u64,
    ytm_fallback: impl std::future::Future<Output = Option<String>>,
) -> Lyrics {
    if let Some(lyrics) = fetch_from_lrclib(title, artist, album, duration_secs).await {
        return lyrics;
    }
    if let Some(text) = ytm_fallback.await.filter(|s| !s.trim().is_empty()) {
        return Lyrics::plain(text, "ytmapi");
    }
    Lyrics::plain("no lyrics found".to_string(), "none")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_centisecond_timestamps() {
        let l = Lyrics::parse_lrc("[00:17.12] hello\n[03:20.45] world\n");
        assert!(l.is_synced);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[0].ms, 17_120);
        assert_eq!(l.lines[0].text, "hello");
        assert_eq!(l.lines[1].ms, 200_450);
        assert_eq!(l.source, "lrclib/synced");
    }

    #[test]
    fn parses_millisecond_and_bare_timestamps() {
        let l = Lyrics::parse_lrc("[03:20.312] ms\n[01:02] bare\n");
        assert!(l.is_synced);
        assert_eq!(l.lines[0].ms, 62_000);
        assert_eq!(l.lines[1].ms, 200_312);
    }

    #[test]
    fn skips_malformed_lines_and_metadata() {
        let l = Lyrics::parse_lrc(
            "[ti:Title]\n[length: 03:45]\n[xx:yy] bad\n[00:10.00] ok\n\nplain line\n",
        );
        assert!(l.is_synced);
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.lines[0].text, "ok");
    }

    #[test]
    fn expands_multiple_timestamps_on_one_line() {
        let l = Lyrics::parse_lrc("[00:10.00][00:20.00] chorus\n");
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[0].ms, 10_000);
        assert_eq!(l.lines[1].ms, 20_000);
        assert_eq!(l.lines[0].text, "chorus");
    }

    #[test]
    fn line_for_returns_last_elapsed() {
        let l = Lyrics::parse_lrc("[00:00.00] a\n[00:10.00] b\n[00:20.00] c\n");
        assert_eq!(l.line_for(0), Some(0));
        assert_eq!(l.line_for(15_000), Some(1));
        assert_eq!(l.line_for(99_000), Some(2));
    }

    #[test]
    fn line_for_returns_none_before_first_line_or_when_empty() {
        let l = Lyrics::parse_lrc("[00:10.00] late\n");
        assert_eq!(l.line_for(9_999), None);
        let empty = Lyrics::parse_lrc("");
        assert_eq!(empty.line_for(0), None);
        assert!(!empty.is_synced);
    }

    #[test]
    fn plain_marks_unsynced() {
        let l = Lyrics::plain("one\ntwo".to_string(), "ytmapi");
        assert!(!l.is_synced);
        assert_eq!(l.lines.len(), 2);
        assert!(l.lines.iter().all(|x| x.ms == 0));
        assert_eq!(l.source, "ytmapi");
        assert_eq!(l.raw, "one\ntwo");
    }

    #[test]
    fn parse_lrc_line_reads_one_tag() {
        assert_eq!(
            parse_lrc_line("[00:17.12] hello"),
            Some((17_120, "hello".to_string()))
        );
        assert_eq!(parse_lrc_line("[ti:Title]"), None);
        assert_eq!(parse_lrc_line("no tag"), None);
    }
}
