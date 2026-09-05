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
use futures::stream::{Stream, StreamExt};
use std::path::{Path, PathBuf};
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::{BrowserToken, OAuthToken};
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, VideoID};
use ytmapi_rs::common::{LikeStatus, TextRun, YoutubeID};
use ytmapi_rs::parse::{
    LibraryArtist, LibraryPlaylist, PlaylistItem, SearchResultAlbum, TableListSong,
};
use ytmapi_rs::query::{
    GetLibraryAlbumsQuery, GetLibraryArtistsQuery, GetLibraryPlaylistsQuery, GetLibrarySongsQuery,
    GetPlaylistTracksQuery,
};
use ytmapi_rs::YtMusic;

const NEEDS_AUTH: &str =
    "not signed in -- run `ytkew --auth cookie` or `ytkew --auth oauth` to reach your library";
const OFFLINE: &str = "no connection to YouTube Music";

/// Ceiling on the items a single paged query will pull. YouTube hands back
/// roughly a hundred per page, so this allows about fifty round trips -- well
/// past any real playlist, but bounded, because a continuation token that
/// keeps pointing at more results would otherwise never stop.
const MAX_ITEMS: usize = 5_000;

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

    /// Albums matching a query, for the search view's album filter.
    pub async fn search_albums(&self, query: &str) -> Result<Vec<AlbumRef>> {
        let res = any_auth!(self, |yt| yt.search_albums(query).await)?;
        Ok(res
            .into_iter()
            .map(|a| AlbumRef {
                id: a.album_id.get_raw().to_string(),
                title: a.title,
                year: if a.artist.is_empty() {
                    a.year
                } else {
                    format!("{} · {}", a.artist, a.year)
                },
            })
            .collect())
    }

    pub async fn search_artists(&self, query: &str) -> Result<Vec<ArtistRef>> {
        let res = any_auth!(self, |yt| yt.search_artists(query).await)?;
        Ok(res
            .into_iter()
            .map(|a| ArtistRef {
                channel_id: a.browse_id.get_raw().to_string(),
                name: a.artist,
                subtitle: a.subscribers.unwrap_or_default(),
            })
            .collect())
    }

    /// Playlists matching a query. YouTube returns featured, community and
    /// podcast results through one filter; podcasts are dropped, since ytkew
    /// has nothing to do with them.
    pub async fn search_playlists(&self, query: &str) -> Result<Vec<Playlist>> {
        use ytmapi_rs::parse::SearchResultPlaylist;
        let res = any_auth!(self, |yt| yt.search_playlists(query).await)?;
        Ok(res
            .into_iter()
            .filter_map(|p| match p {
                SearchResultPlaylist::Featured(f) => Some(Playlist {
                    id: f.playlist_id.get_raw().to_string(),
                    title: f.title,
                    author: f.author,
                    track_count: f.songs,
                }),
                SearchResultPlaylist::Community(c) => Some(Playlist {
                    id: c.playlist_id.get_raw().to_string(),
                    title: c.title,
                    author: c.author,
                    track_count: c.views,
                }),
                // Podcasts, and whatever else upstream adds to this
                // non-exhaustive enum, are not things ytkew plays.
                _ => None,
            })
            .collect())
    }

    /// Videos matching a query. Music videos and live sets live here rather
    /// than under songs, and they play the same way.
    pub async fn search_videos(&self, query: &str) -> Result<Vec<Track>> {
        use ytmapi_rs::parse::SearchResultVideo;
        let res = any_auth!(self, |yt| yt.search_videos(query).await)?;
        Ok(res
            .into_iter()
            .filter_map(|v| match v {
                SearchResultVideo::Video {
                    title,
                    channel_name,
                    video_id,
                    length,
                    thumbnails,
                    ..
                } => Some(Track {
                    video_id: video_id.get_raw().to_string(),
                    title,
                    artist: channel_name,
                    album: None,
                    duration: crate::model::parse_duration(&length),
                    duration_text: length,
                    thumbnail: crate::model::best_thumbnail(&thumbnails, 544),
                }),
                // Podcast episodes are not playable through this path.
                SearchResultVideo::VideoEpisode { .. } => None,
            })
            .collect())
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

    /// Every track on a playlist, page by page.
    ///
    /// YouTube returns about a hundred tracks per response and a token for the
    /// rest; `on_page` is called with each page as it arrives so a long
    /// playlist fills in progressively instead of appearing all at once at the
    /// end.
    pub async fn playlist_tracks_paged(
        &self,
        playlist_id: &str,
        mut on_page: impl FnMut(Vec<Track>),
    ) -> Result<()> {
        let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(browse_id(playlist_id)));
        any_auth!(self, |yt| drain(
            yt.stream(&query),
            |page: Vec<PlaylistItem>| {
                on_page(page.iter().filter_map(track_from_playlist_item).collect())
            }
        )
        .await)
    }

    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        self.playlist_tracks_paged(playlist_id, |page| out.extend(page))
            .await?;
        Ok(out)
    }

    /// Songs you've liked in YouTube Music. This is the `LM` playlist, which
    /// is a different thing from `get_library_songs` (tracks explicitly added
    /// to your library) -- an account commonly has liked songs and no library
    /// songs at all.
    pub async fn liked_songs_paged(&self, on_page: impl FnMut(Vec<Track>)) -> Result<()> {
        self.playlist_tracks_paged("LM", on_page).await
    }

    pub async fn liked_songs(&self) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        self.liked_songs_paged(|page| out.extend(page)).await?;
        Ok(out)
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

    pub async fn library_playlists_paged(
        &self,
        mut on_page: impl FnMut(Vec<Playlist>),
    ) -> Result<()> {
        let query = GetLibraryPlaylistsQuery;
        let res = logged_in!(self, |yt| drain(
            yt.stream(&query),
            |page: Vec<LibraryPlaylist>| {
                on_page(
                    page.into_iter()
                        .map(|p| Playlist {
                            id: p.playlist_id.get_raw().to_string(),
                            title: p.title,
                            author: p.author,
                            track_count: p.tracks,
                        })
                        .collect(),
                )
            }
        )
        .await);
        ignore_missing_shelf(res)
    }

    pub async fn library_playlists(&self) -> Result<Vec<Playlist>> {
        let mut out = Vec::new();
        self.library_playlists_paged(|page| out.extend(page))
            .await?;
        Ok(out)
    }

    pub async fn library_songs_paged(&self, mut on_page: impl FnMut(Vec<Track>)) -> Result<()> {
        let query = GetLibrarySongsQuery::default();
        let res = logged_in!(self, |yt| drain(
            yt.stream(&query),
            |page: Vec<TableListSong>| on_page(page.iter().map(Track::from).collect())
        )
        .await);
        ignore_missing_shelf(res)
    }

    pub async fn library_songs(&self) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        self.library_songs_paged(|page| out.extend(page)).await?;
        Ok(out)
    }

    pub async fn library_albums_paged(&self, mut on_page: impl FnMut(Vec<AlbumRef>)) -> Result<()> {
        let query = GetLibraryAlbumsQuery::default();
        let res = logged_in!(self, |yt| drain(
            yt.stream(&query),
            |page: Vec<SearchResultAlbum>| {
                on_page(
                    page.into_iter()
                        .map(|a| AlbumRef {
                            id: a.album_id.get_raw().to_string(),
                            title: a.title,
                            year: a.year,
                        })
                        .collect(),
                )
            }
        )
        .await);
        ignore_missing_shelf(res)
    }

    pub async fn library_albums(&self) -> Result<Vec<AlbumRef>> {
        let mut out = Vec::new();
        self.library_albums_paged(|page| out.extend(page)).await?;
        Ok(out)
    }

    pub async fn library_artists_paged(
        &self,
        mut on_page: impl FnMut(Vec<ArtistRef>),
    ) -> Result<()> {
        let query = GetLibraryArtistsQuery::default();
        let res = logged_in!(self, |yt| drain(
            yt.stream(&query),
            |page: Vec<LibraryArtist>| {
                on_page(
                    page.into_iter()
                        .map(|a| ArtistRef {
                            channel_id: a.channel_id.get_raw().to_string(),
                            name: a.artist,
                            subtitle: a.byline,
                        })
                        .collect(),
                )
            }
        )
        .await);
        ignore_missing_shelf(res)
    }

    pub async fn library_artists(&self) -> Result<Vec<ArtistRef>> {
        let mut out = Vec::new();
        self.library_artists_paged(|page| out.extend(page)).await?;
        Ok(out)
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

/// Walk every page of a continuation stream, handing each to `on_page` as it
/// lands so callers can show early results rather than waiting for the whole
/// fetch.
///
/// Once one page has succeeded a later failure ends the walk quietly. A
/// truncated playlist beats throwing away everything already retrieved
/// because one request deep into a long fetch timed out.
async fn drain<S, T, E>(stream: S, mut on_page: impl FnMut(Vec<T>)) -> Result<()>
where
    S: Stream<Item = std::result::Result<Vec<T>, E>>,
    E: std::fmt::Display,
{
    let mut stream = std::pin::pin!(stream);
    let mut pages = 0usize;
    let mut taken = 0usize;
    while let Some(page) = stream.next().await {
        let mut items = match page {
            Ok(items) => items,
            Err(e) if pages == 0 => return Err(anyhow!(e.to_string())),
            Err(_) => break,
        };
        pages += 1;
        items.truncate(MAX_ITEMS - taken);
        taken += items.len();
        if !items.is_empty() {
            on_page(items);
        }
        if taken >= MAX_ITEMS {
            break;
        }
    }
    Ok(())
}

/// An empty library section makes YouTube omit the shelf entirely, which
/// ytmapi-rs reports as a missing-key parse error. Treat that specific shape
/// as "nothing here" rather than showing the user a raw JSON path.
fn is_missing_shelf(msg: &str) -> bool {
    msg.contains("not found in Api response")
        && (msg.contains("musicShelfRenderer") || msg.contains("gridRenderer"))
}

fn empty_on_missing_shelf<T: Default>(res: Result<T, ytmapi_rs::Error>) -> Result<T> {
    match res {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = e.to_string();
            if is_missing_shelf(&msg) {
                Ok(T::default())
            } else {
                Err(anyhow!(msg))
            }
        }
    }
}

/// The paged equivalent: an absent shelf means the section is empty, not that
/// the fetch failed.
fn ignore_missing_shelf(res: Result<()>) -> Result<()> {
    match res {
        Err(e) if is_missing_shelf(&e.to_string()) => Ok(()),
        other => other,
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

    /// A stream of canned pages, standing in for YouTube's continuations.
    fn pages<T: Clone>(
        pages: Vec<std::result::Result<Vec<T>, &'static str>>,
    ) -> impl Stream<Item = std::result::Result<Vec<T>, &'static str>> {
        futures::stream::iter(pages)
    }

    #[tokio::test]
    async fn every_page_is_delivered_in_order() {
        let mut seen = Vec::new();
        drain(
            pages(vec![Ok(vec![1, 2]), Ok(vec![3]), Ok(vec![4, 5])]),
            |p| seen.push(p),
        )
        .await
        .unwrap();
        assert_eq!(seen, vec![vec![1, 2], vec![3], vec![4, 5]]);
    }

    #[tokio::test]
    async fn a_single_page_is_not_treated_as_truncated() {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        drain(pages(vec![Ok(vec![1, 2, 3])]), |p| seen.push(p))
            .await
            .unwrap();
        assert_eq!(seen, vec![vec![1, 2, 3]]);
    }

    #[tokio::test]
    async fn an_empty_result_yields_no_pages_but_still_succeeds() {
        let mut calls = 0;
        drain(pages::<u8>(vec![Ok(Vec::new())]), |_| calls += 1)
            .await
            .unwrap();
        assert_eq!(calls, 0, "an empty page is not worth forwarding");
    }

    #[tokio::test]
    async fn a_failure_on_the_first_page_is_an_error() {
        let mut calls = 0;
        let e = drain(pages::<u8>(vec![Err("boom")]), |_| calls += 1)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("boom"), "got {e}");
        assert_eq!(calls, 0);
    }

    #[tokio::test]
    async fn a_failure_later_keeps_what_already_arrived() {
        let mut seen = Vec::new();
        // Losing a whole playlist to one hiccup deep in the fetch is worse
        // than returning the part that did arrive.
        drain(
            pages(vec![Ok(vec![1, 2]), Err("timeout"), Ok(vec![9])]),
            |p| seen.push(p),
        )
        .await
        .unwrap();
        assert_eq!(seen, vec![vec![1, 2]]);
    }

    #[tokio::test]
    async fn the_item_cap_bounds_a_runaway_continuation() {
        let page: Vec<u8> = vec![0; 1000];
        let endless = std::iter::repeat_n(Ok(page), 100).collect();
        let mut total = 0usize;
        drain(pages(endless), |p| total += p.len()).await.unwrap();
        assert_eq!(total, MAX_ITEMS);
    }

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
