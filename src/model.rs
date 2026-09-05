//! Normalized track model. The API hands back half a dozen different shapes
//! (search results, playlist items, watch playlist tracks); everything gets
//! funneled into `Track` so the UI and queue only ever see one type.

use ytmapi_rs::common::{Thumbnail, YoutubeID};
use ytmapi_rs::parse::{PlaylistItem, SearchResultSong, WatchPlaylistTrack};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Duration in seconds, when the API bothered to tell us.
    pub duration: Option<f64>,
    pub duration_text: String,
    pub thumbnail: Option<String>,
}

impl Track {
    pub fn url(&self) -> String {
        format!("https://music.youtube.com/watch?v={}", self.video_id)
    }

    /// "Artist - Title", the form kew uses for window titles and notifications.
    pub fn display(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }
}

/// Parse "3:59" or "1:02:03" into seconds.
pub fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut secs = 0f64;
    for p in &parts {
        let n: f64 = p.trim().parse().ok()?;
        secs = secs * 60.0 + n;
    }
    Some(secs)
}

pub fn fmt_duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "0:00".into();
    }
    let total = secs as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// YouTube thumbnail URLs end in a size directive like `=w60-h60-l90-rj`.
/// Search results only ever offer 60px and 120px, which is useless for a
/// terminal cover, but rewriting the directive gets us the full-size image.
pub fn upscale_thumbnail(url: &str, size: u32) -> String {
    let Some(eq) = url.rfind('=') else {
        return url.to_string();
    };
    let (base, directive) = url.split_at(eq);
    let rest = &directive[1..];
    // Only rewrite if it actually looks like a size directive (w<n>-h<n>-...).
    if !rest.starts_with('w') {
        return url.to_string();
    }
    let tail: Vec<&str> = rest
        .split('-')
        .filter(|seg| !(seg.starts_with('w') || seg.starts_with('h')))
        .collect();
    let mut out = format!("{base}=w{size}-h{size}");
    for seg in tail {
        out.push('-');
        out.push_str(seg);
    }
    out
}

/// Pick the biggest thumbnail on offer, then ask YouTube for a bigger one.
pub fn best_thumbnail(thumbs: &[Thumbnail], size: u32) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| t.width * t.height)
        .map(|t| upscale_thumbnail(&t.url, size))
}

const COVER_PX: u32 = 544;

impl From<&SearchResultSong> for Track {
    fn from(s: &SearchResultSong) -> Self {
        Track {
            video_id: s.video_id.get_raw().to_string(),
            title: s.title.clone(),
            artist: s.artist.clone(),
            album: s.album.as_ref().map(|a| a.name.clone()),
            duration: parse_duration(&s.duration),
            duration_text: s.duration.clone(),
            thumbnail: best_thumbnail(&s.thumbnails, COVER_PX),
        }
    }
}

impl From<&WatchPlaylistTrack> for Track {
    fn from(t: &WatchPlaylistTrack) -> Self {
        Track {
            video_id: t.video_id.get_raw().to_string(),
            title: t.title.clone(),
            artist: t.author.clone(),
            album: None,
            duration: parse_duration(&t.duration),
            duration_text: t.duration.clone(),
            thumbnail: best_thumbnail(&t.thumbnails, COVER_PX),
        }
    }
}

/// Playlist items come in four flavors; only songs and videos are playable
/// music, and podcast episodes are deliberately dropped.
pub fn track_from_playlist_item(item: &PlaylistItem) -> Option<Track> {
    match item {
        PlaylistItem::Song(s) => Some(Track {
            video_id: s.video_id.get_raw().to_string(),
            title: s.title.clone(),
            artist: s
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            album: Some(s.album.name.clone()),
            duration: parse_duration(&s.duration),
            duration_text: s.duration.clone(),
            thumbnail: best_thumbnail(&s.thumbnails, COVER_PX),
        }),
        PlaylistItem::Video(v) => Some(Track {
            video_id: v.video_id.get_raw().to_string(),
            title: v.title.clone(),
            artist: v
                .channel_name
                .clone(),
            album: None,
            duration: parse_duration(&v.duration),
            duration_text: v.duration.clone(),
            thumbnail: best_thumbnail(&v.thumbnails, COVER_PX),
        }),
        _ => None,
    }
}

impl From<&ytmapi_rs::parse::TableListSong> for Track {
    fn from(s: &ytmapi_rs::parse::TableListSong) -> Self {
        Track {
            video_id: s.video_id.get_raw().to_string(),
            title: s.title.clone(),
            artist: s
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            album: Some(s.album.name.clone()),
            duration: parse_duration(&s.duration),
            duration_text: s.duration.clone(),
            thumbnail: best_thumbnail(&s.thumbnails, COVER_PX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_round_trip() {
        assert_eq!(parse_duration("3:59"), Some(239.0));
        assert_eq!(parse_duration("1:02:03"), Some(3723.0));
        assert_eq!(parse_duration(""), None);
        assert_eq!(fmt_duration(239.0), "3:59");
        assert_eq!(fmt_duration(3723.0), "1:02:03");
    }

    #[test]
    fn thumbnail_upscaling_rewrites_size_directive() {
        let u = "https://yt3.googleusercontent.com/abc=w60-h60-l90-rj";
        assert_eq!(
            upscale_thumbnail(u, 544),
            "https://yt3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
    }

    #[test]
    fn thumbnail_upscaling_leaves_odd_urls_alone() {
        let u = "https://example.com/cover.jpg";
        assert_eq!(upscale_thumbnail(u, 544), u);
    }
}

