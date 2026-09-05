//! YouTube Music API layer.
//!
//! `ytmapi-rs` parameterizes its handle over the auth token type, and the
//! `AuthToken` trait is not object-safe (it returns `impl IntoIterator` and
//! carries a generic associated function), so a `Box<dyn AuthToken>` is out.
//! Instead we hold a three-way enum and dispatch through macros.
//!
//! The split matters for UX: the crate gates *library* queries behind a
//! `LoggedIn` marker trait, while search, playlist reads, radio and lyrics
//! work with no credentials at all. So `ytkew <query>` is useful before the
//! user has set anything up, and only "my playlists" demands a cookie.

use crate::model::{track_from_playlist_item, Track};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::{BrowserToken, OAuthToken};
use ytmapi_rs::common::{LikeStatus, TextRun, YoutubeID};
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, VideoID};
use ytmapi_rs::YtMusic;

const NEEDS_AUTH: &str =
    "not signed in -- run `ytkew --auth cookie` or `ytkew --auth oauth` to reach your library";
const OFFLINE: &str = "no connection to YouTube Music";

enum Backend {
    /// Could not reach YouTube Music at startup. Every query fails with a
    /// clear message instead of the process refusing to start.
    Offline,
    NoAuth(YtMusic<NoAuthToken>),
    Browser(YtMusic<BrowserToken>),
    OAuth(YtMusic<OAuthToken>),
}

/// A playlist as it appears in the library browser.
#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub author: String,
    pub track_count: String,
}

/// An artist in the library browser.
#[derive(Clone, Debug)]
pub struct ArtistRef {
    pub channel_id: String,
    pub name: String,
    pub subtitle: String,
}

/// An album, either from the library or from an artist's discography.
#[derive(Clone, Debug)]
pub struct AlbumRef {
    pub id: String,
    pub title: String,
    pub year: String,
}

pub struct Api {
    backend: Backend,
}

/// Dispatch a call available on any auth level.
macro_rules! any_auth {
    ($self:ident, |$yt:ident| $body:expr) => {
        match &$self.backend {
            Backend::Offline => return Err(anyhow!(OFFLINE)),
            Backend::NoAuth($yt) => $body,
            Backend::Browser($yt) => $body,
            Backend::OAuth($yt) => $body,
        }
    };
}

/// Dispatch a call that requires credentials, erroring helpfully if absent.
macro_rules! logged_in {
    ($self:ident, |$yt:ident| $body:expr) => {
        match &$self.backend {
            Backend::Offline => return Err(anyhow!(OFFLINE)),
            Backend::NoAuth(_) => return Err(anyhow!(NEEDS_AUTH)),
            Backend::Browser($yt) => $body,
            Backend::OAuth($yt) => $body,
        }
    };
}

impl Api {
    /// Try cookie auth, then OAuth, then fall back to unauthenticated so the
    /// app still starts and can still search.
    pub async fn connect(config_dir: &Path) -> (Self, Option<String>) {
        let cookie = config_dir.join("cookie.txt");
        let oauth = config_dir.join("oauth.json");

        // Retro-tighten permissions on credentials written by older versions.
        for f in [&cookie, &oauth] {
            harden_permissions(f);
        }

        if cookie.exists() {
            match YtMusic::from_cookie_file(&cookie).await {
                Ok(yt) => {
                    return (
                        Self {
                            backend: Backend::Browser(yt),
                        },
                        None,
                    )
                }
                Err(e) => {
                    let warn = format!("cookie auth failed ({e}); continuing unauthenticated");
                    return (Self::unauthenticated().await, Some(warn));
                }
            }
        }

        if oauth.exists() {
            match Self::from_oauth_file(&oauth).await {
                Ok(api) => return (api, None),
                Err(e) => {
                    let warn = format!("oauth failed ({e}); continuing unauthenticated");
                    return (Self::unauthenticated().await, Some(warn));
                }
            }
        }

        (
            Self::unauthenticated().await,
            Some("no credentials found -- search works, library needs `ytkew auth`".into()),
        )
    }

    async fn unauthenticated() -> Self {
        // Failing here means no network. Start anyway in an offline state --
        // the user can still see the UI and read the error, and retrying is
        // just a restart away.
        match YtMusic::new_unauthenticated().await {
            Ok(yt) => Self {
                backend: Backend::NoAuth(yt),
            },
            Err(_) => Self {
                backend: Backend::Offline,
            },
        }
    }

    async fn from_oauth_file(path: &Path) -> Result<Self> {
        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        let token: OAuthToken =
            serde_json::from_str(&raw).context("oauth.json is not a valid saved token")?;
        let mut yt = YtMusic::from_auth_token(token);
        // Access tokens last minutes, so refresh on load and persist the new
        // one; otherwise every restart would fail its first query.
        if let Ok(fresh) = yt.refresh_token().await {
            if let Ok(json) = serde_json::to_string_pretty(&fresh) {
                let _ = tokio::fs::write(path, json).await;
            }
        }
        Ok(Self {
            backend: Backend::OAuth(yt),
        })
    }

    pub fn is_authenticated(&self) -> bool {
        matches!(self.backend, Backend::Browser(_) | Backend::OAuth(_))
    }

    pub fn is_offline(&self) -> bool {
        matches!(self.backend, Backend::Offline)
    }

    // --- unauthenticated-capable queries ---------------------------------

    pub async fn search_songs(&self, query: &str) -> Result<Vec<Track>> {
        let res = any_auth!(self, |yt| yt.search_songs(query).await)?;
        Ok(res.iter().map(Track::from).collect())
    }

    pub async fn search_suggestions(&self, query: &str) -> Result<Vec<String>> {
        let res = any_auth!(self, |yt| yt.get_search_suggestions(query).await)?;
        Ok(res
            .iter()
            .map(|s| {
                s.runs
                    .iter()
                    .map(|r| match r {
                        TextRun::Bold(t) | TextRun::Normal(t) => t.as_str(),
                    })
                    .collect::<String>()
            })
            .collect())
    }

    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>> {
        let id = PlaylistID::from_raw(browse_id(playlist_id));
        let res = any_auth!(self, |yt| yt.get_playlist_tracks(id.clone()).await)?;
        Ok(res.iter().filter_map(track_from_playlist_item).collect())
    }




    /// Songs you've liked in YouTube Music. This is the `LM` playlist, which
    /// is a different thing from `get_library_songs` (tracks explicitly added
    /// to your library) -- an account commonly has liked songs and no library
    /// songs at all.
    pub async fn liked_songs(&self) -> Result<Vec<Track>> {
        self.playlist_tracks("LM").await
    }

    /// YouTube's "start radio from this track" -- an endless-ish mix. This is
    /// what makes `ytkew <artist>` behave like kew's auto-generated playlists
    /// rather than playing exactly one song and stopping.
    pub async fn radio_from(&self, video_id: &str) -> Result<Vec<Track>> {
        let id = VideoID::from_raw(video_id);
        let res = any_auth!(self, |yt| yt
            .get_watch_playlist_from_video_id(id.clone())
            .await)?;
        Ok(res.iter().map(Track::from).collect())
    }

    pub async fn lyrics(&self, video_id: &str) -> Result<String> {
        let id = VideoID::from_raw(video_id);
        let lyrics_id = any_auth!(self, |yt| yt.get_lyrics_id(id.clone()).await)?;
        let lyrics = any_auth!(self, |yt| yt.get_lyrics(lyrics_id.clone()).await)?;
        Ok(lyrics.lyrics)
    }

    // --- queries that need credentials -----------------------------------

    pub async fn library_playlists(&self) -> Result<Vec<Playlist>> {
        let res = logged_in!(self, |yt| yt.get_library_playlists().await)?;
        Ok(res
            .into_iter()
            .map(|p| Playlist {
                id: p.playlist_id.get_raw().to_string(),
                title: p.title,
                author: p.author,
                track_count: p.tracks,
            })
            .collect())
    }

    pub async fn library_songs(&self) -> Result<Vec<Track>> {
        let res = logged_in!(self, |yt| yt.get_library_songs().await);
        Ok(empty_on_missing_shelf(res)?
            .iter()
            .map(Track::from)
            .collect())
    }

    pub async fn library_albums(&self) -> Result<Vec<AlbumRef>> {
        let res = logged_in!(self, |yt| yt.get_library_albums().await);
        Ok(empty_on_missing_shelf(res)?
            .into_iter()
            .map(|a| AlbumRef {
                id: a.album_id.get_raw().to_string(),
                title: a.title,
                year: a.year,
            })
            .collect())
    }

    pub async fn library_artists(&self) -> Result<Vec<ArtistRef>> {
        let res = logged_in!(self, |yt| yt.get_library_artists().await)?;
        Ok(res
            .into_iter()
            .map(|a| ArtistRef {
                channel_id: a.channel_id.get_raw().to_string(),
                name: a.artist,
                subtitle: a.byline,
            })
            .collect())
    }

    /// An artist's albums and singles, as shown on their YouTube Music page.
    pub async fn artist_albums(&self, channel_id: &str) -> Result<Vec<AlbumRef>> {
        let id = ArtistChannelID::from_raw(channel_id);
        let artist = any_auth!(self, |yt| yt.get_artist(id.clone()).await)?;
        let mut out = Vec::new();
        // Albums and singles are separate shelves; both are worth browsing.
        for shelf in [&artist.top_releases.albums, &artist.top_releases.singles]
            .into_iter()
            .flatten()
        {
            for a in &shelf.results {
                out.push(AlbumRef {
                    id: a.album_id.get_raw().to_string(),
                    title: a.title.clone(),
                    year: a.year.clone(),
                });
            }
        }
        Ok(out)
    }

    /// Tracks on an album. `AlbumSong` carries no artist or artwork of its
    /// own, so those are filled in from the album itself.
    pub async fn album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        let id = AlbumID::from_raw(album_id);
        let album = any_auth!(self, |yt| yt.get_album(id.clone()).await)?;
        let artist = album
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let art = crate::model::best_thumbnail(&album.thumbnails, 544);
        Ok(album
            .tracks
            .iter()
            .map(|t| Track {
                video_id: t.video_id.get_raw().to_string(),
                title: t.title.clone(),
                artist: artist.clone(),
                album: Some(album.title.clone()),
                duration: crate::model::parse_duration(&t.duration),
                duration_text: t.duration.clone(),
                thumbnail: art.clone(),
            })
            .collect())
    }

    pub async fn history_count(&self) -> Result<usize> {
        let res = logged_in!(self, |yt| yt.get_history().await);
        Ok(empty_on_missing_shelf(res)?.len())
    }

    pub async fn rate(&self, video_id: &str, status: LikeStatus) -> Result<()> {
        let id = VideoID::from_raw(video_id);
        logged_in!(self, |yt| yt.rate_song(id.clone(), status).await)?;
        Ok(())
    }
}

/// Make a credential file owner-only if it isn't already.
fn harden_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = std::fs::metadata(path) {
            if md.permissions().mode() & 0o077 != 0 {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// YouTube's browse endpoint wants a playlist addressed as `VL<playlistId>`.
/// ytmapi-rs passes the id through untouched (it flags this as a TODO in
/// `query/playlist.rs`), so a bare id like `LM` comes back HTTP 400. Prepend
/// the prefix unless the caller already did.
fn browse_id(playlist_id: &str) -> String {
    if playlist_id.starts_with("VL") {
        playlist_id.to_string()
    } else {
        format!("VL{playlist_id}")
    }
}

/// An empty library section makes YouTube omit the shelf entirely, which
/// ytmapi-rs reports as a missing-key parse error. Treat that specific shape
/// as "nothing here" rather than showing the user a raw JSON path.
fn empty_on_missing_shelf<T: Default>(res: Result<T, ytmapi_rs::Error>) -> Result<T> {
    match res {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found in Api response")
                && (msg.contains("musicShelfRenderer") || msg.contains("gridRenderer"))
            {
                Ok(T::default())
            } else {
                Err(anyhow!(msg))
            }
        }
    }
}

/// Where cookie.txt / oauth.json / config.toml live.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ytkew")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ytkew")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_ids_get_the_vl_browse_prefix() {
        // YouTube's browse endpoint 400s on a bare playlist id.
        assert_eq!(browse_id("LM"), "VLLM");
        assert_eq!(browse_id("PLabc123"), "VLPLabc123");
    }

    #[test]
    fn an_already_prefixed_id_is_left_alone() {
        assert_eq!(browse_id("VLLM"), "VLLM");
        assert_eq!(browse_id("VLPLabc123"), "VLPLabc123");
    }

    impl Api {
        fn offline_for_test() -> Self {
            Self {
                backend: Backend::Offline,
            }
        }
    }

    #[tokio::test]
    async fn offline_queries_error_instead_of_panicking() {
        let api = Api::offline_for_test();
        assert!(!api.is_authenticated());
        assert!(api.is_offline());

        let e = api.search_songs("anything").await.unwrap_err();
        assert!(e.to_string().contains("no connection"), "got {e}");

        // Library queries report offline too, not the misleading auth hint.
        let e = api.library_playlists().await.unwrap_err();
        assert!(e.to_string().contains("no connection"), "got {e}");

        let e = api.radio_from("abc").await.unwrap_err();
        assert!(e.to_string().contains("no connection"), "got {e}");
    }
}

