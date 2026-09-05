//! Search, autocomplete, radio, lyrics and ratings -- everything that
//! reaches the API on the user's behalf.

use super::*;
use crate::api::{AlbumRef, ArtistRef, Playlist};

/// Which kind of result the search view is showing.
///
/// YouTube Music filters server-side rather than returning one mixed list, so
/// this picks the endpoint as much as the presentation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchFilter {
    #[default]
    Songs,
    Videos,
    Albums,
    Artists,
    Playlists,
}

pub const SEARCH_FILTERS: [SearchFilter; 5] = [
    SearchFilter::Songs,
    SearchFilter::Videos,
    SearchFilter::Albums,
    SearchFilter::Artists,
    SearchFilter::Playlists,
];

impl SearchFilter {
    pub fn name(self) -> &'static str {
        match self {
            SearchFilter::Songs => "songs",
            SearchFilter::Videos => "videos",
            SearchFilter::Albums => "albums",
            SearchFilter::Artists => "artists",
            SearchFilter::Playlists => "playlists",
        }
    }

    pub fn step(self, delta: i32) -> Self {
        let n = SEARCH_FILTERS.len() as i32;
        let i = SEARCH_FILTERS.iter().position(|f| *f == self).unwrap_or(0) as i32;
        SEARCH_FILTERS[((((i + delta) % n) + n) % n) as usize]
    }
}

enum SearchTarget {
    Album(String),
    Playlist(String),
}

/// One row of search results. Songs and videos play; the rest are things you
/// step into.
#[derive(Clone, Debug)]
pub enum SearchHit {
    Song(Track),
    Video(Track),
    Album(AlbumRef),
    Artist(ArtistRef),
    Playlist(Playlist),
}

impl SearchHit {
    /// The track this row plays, if it plays one at all.
    pub fn track(&self) -> Option<&Track> {
        match self {
            SearchHit::Song(t) | SearchHit::Video(t) => Some(t),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            SearchHit::Song(t) | SearchHit::Video(t) => t.title.clone(),
            SearchHit::Album(a) => a.title.clone(),
            SearchHit::Artist(a) => a.name.clone(),
            SearchHit::Playlist(p) => p.title.clone(),
        }
    }

    pub fn sublabel(&self) -> String {
        match self {
            SearchHit::Song(t) | SearchHit::Video(t) => t.artist.clone(),
            SearchHit::Album(a) => a.year.clone(),
            SearchHit::Artist(a) => a.subtitle.clone(),
            SearchHit::Playlist(p) => {
                if p.author.is_empty() {
                    p.track_count.clone()
                } else {
                    format!("{} · {}", p.author, p.track_count)
                }
            }
        }
    }

    /// Right-hand column: a duration for playable rows, a type for the rest,
    /// so a mixed-looking list still says what each row is.
    pub fn trailing(&self) -> String {
        match self {
            SearchHit::Song(t) | SearchHit::Video(t) => t.duration_text.clone(),
            SearchHit::Album(_) => "album".into(),
            SearchHit::Artist(_) => "artist".into(),
            SearchHit::Playlist(_) => "playlist".into(),
        }
    }
}

impl App {
    /// Ask for autocomplete on the current input. Cheap enough to fire per
    /// keystroke; stale replies are discarded by comparing the query back.
    pub fn refresh_suggestions(&mut self) {
        let q = self.search_input.trim().to_string();
        if q.len() < 2 {
            self.suggestions.clear();
            return;
        }
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Ok(items) = api.search_suggestions(&q).await {
                let _ = tx.send(AppMsg::Suggestions { query: q, items });
            }
        });
    }

    // --- search / radio / lyrics / likes ---------------------------------

    pub fn submit_search(&mut self) {
        let q = self.search_input.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.search_editing = false;
        self.suggestions.clear();
        self.run_search(q, self.search_filter);
    }

    /// Re-run the current query under a different filter.
    pub(crate) fn cycle_search_filter(&mut self, delta: i32) {
        self.search_filter = self.search_filter.step(delta);
        let q = self.search_input.trim().to_string();
        if q.is_empty() {
            self.search_results.clear();
            return;
        }
        self.run_search(q, self.search_filter);
    }

    /// Fire the query for one filter. The reply carries the filter back, so a
    /// slow answer cannot land in a list the user has already switched away
    /// from.
    fn run_search(&mut self, q: String, filter: SearchFilter) {
        self.searching = true;
        self.search_sel = 0;
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let hits = match filter {
                SearchFilter::Songs => api
                    .search_songs(&q)
                    .await
                    .map(|v| v.into_iter().map(SearchHit::Song).collect()),
                SearchFilter::Videos => api
                    .search_videos(&q)
                    .await
                    .map(|v| v.into_iter().map(SearchHit::Video).collect()),
                SearchFilter::Albums => api
                    .search_albums(&q)
                    .await
                    .map(|v| v.into_iter().map(SearchHit::Album).collect()),
                SearchFilter::Artists => api
                    .search_artists(&q)
                    .await
                    .map(|v| v.into_iter().map(SearchHit::Artist).collect()),
                SearchFilter::Playlists => api
                    .search_playlists(&q)
                    .await
                    .map(|v| v.into_iter().map(SearchHit::Playlist).collect()),
            };
            let msg = match hits {
                Ok(hits) => AppMsg::SearchResults { filter, hits },
                Err(e) => AppMsg::Error(format!("search: {e}")),
            };
            let _ = tx.send(msg);
        });
    }

    /// Act on the highlighted result.
    ///
    /// Songs and videos play; a playlist or album becomes the new queue, the
    /// way opening one in YouTube Music does; an artist drills down into
    /// their releases.
    pub(crate) fn activate_search_hit(&mut self, _jump: bool) {
        let Some(hit) = self.search_results.get(self.search_sel).cloned() else {
            return;
        };
        match hit {
            SearchHit::Song(t) | SearchHit::Video(t) => self.play_one_off(t),
            SearchHit::Album(a) => {
                self.notify(format!("loading {}", a.title));
                self.load_and_play(SearchTarget::Album(a.id));
            }
            SearchHit::Playlist(p) => {
                self.notify(format!("loading {}", p.title));
                self.load_and_play(SearchTarget::Playlist(p.id));
            }
            SearchHit::Artist(a) => {
                // No tracks of its own, so show what the artist has instead
                // of guessing which release you meant.
                self.notify(format!("{}: releases", a.name));
                self.load_artist_releases(a.channel_id);
            }
        }
    }

    /// Fetch a container's tracks and make them the queue.
    fn load_and_play(&mut self, target: SearchTarget) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let tracks = match &target {
                SearchTarget::Album(id) => api.album_tracks(id).await,
                SearchTarget::Playlist(id) => api.playlist_tracks(id).await,
            };
            let msg = match tracks {
                Ok(tracks) if tracks.is_empty() => AppMsg::Error("nothing playable there".into()),
                Ok(tracks) => AppMsg::PlayCollection(tracks),
                Err(e) => AppMsg::Error(format!("{e}")),
            };
            let _ = tx.send(msg);
        });
    }

    /// Replace the results with an artist's albums and singles.
    fn load_artist_releases(&mut self, channel_id: String) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        let filter = self.search_filter;
        self.searching = true;
        tokio::spawn(async move {
            let msg = match api.artist_albums(&channel_id).await {
                Ok(albums) => AppMsg::SearchResults {
                    filter,
                    hits: albums.into_iter().map(SearchHit::Album).collect(),
                },
                Err(e) => AppMsg::Error(format!("{e}")),
            };
            let _ = tx.send(msg);
        });
    }

    /// Extend the queue with YouTube's radio mix for a track, which is how a
    /// one-song search turns into a listening session.
    pub fn append_radio(&mut self, video_id: &str) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        let vid = video_id.to_string();
        tokio::spawn(async move {
            if let Ok(tracks) = api.radio_from(&vid).await {
                let _ = tx.send(AppMsg::RadioTail { after: vid, tracks });
            }
        });
    }

    pub(crate) fn radio_from_current(&mut self) {
        if let Some(t) = self.queue.current().cloned() {
            self.notify("starting radio");
            self.append_radio(&t.video_id);
        }
    }

    pub fn ensure_lyrics(&mut self) {
        let Some(track) = self.queue.current().cloned() else {
            return;
        };
        if self.lyrics_for.as_deref() == Some(track.video_id.as_str()) {
            return;
        }
        let api = self.api.clone();
        let tx = self.tx.clone();
        let vid = track.video_id.clone();
        tokio::spawn(async move {
            match api.lyrics(&vid).await {
                Ok(text) => {
                    let _ = tx.send(AppMsg::Lyrics {
                        video_id: vid,
                        text,
                    });
                }
                Err(_) => {
                    let _ = tx.send(AppMsg::Lyrics {
                        video_id: vid,
                        text: "no lyrics found".into(),
                    });
                }
            }
        });
    }

    pub(crate) fn like_current(&mut self) {
        let Some(track) = self.queue.current().cloned() else {
            return;
        };
        if !self.api.is_authenticated() {
            self.notify("liking needs auth");
            return;
        }
        let api = self.api.clone();
        let tx = self.tx.clone();
        self.notify(format!("liked {}", track.title));
        tokio::spawn(async move {
            if let Err(e) = api
                .rate(&track.video_id, ytmapi_rs::common::LikeStatus::Liked)
                .await
            {
                let _ = tx.send(AppMsg::Error(format!("like failed: {e}")));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: &str) -> Track {
        Track {
            video_id: "v".into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration: None,
            duration_text: "3:20".into(),
            thumbnail: None,
        }
    }

    #[test]
    fn the_filter_wraps_in_both_directions() {
        assert_eq!(SearchFilter::Songs.step(1), SearchFilter::Videos);
        assert_eq!(SearchFilter::Songs.step(-1), SearchFilter::Playlists);
        assert_eq!(SearchFilter::Playlists.step(1), SearchFilter::Songs);
        // A full lap returns where it started.
        let mut f = SearchFilter::Songs;
        for _ in 0..SEARCH_FILTERS.len() {
            f = f.step(1);
        }
        assert_eq!(f, SearchFilter::Songs);
    }

    #[test]
    fn every_filter_has_a_distinct_name() {
        let mut names: Vec<_> = SEARCH_FILTERS.iter().map(|f| f.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before);
    }

    #[test]
    fn only_songs_and_videos_are_playable() {
        assert!(SearchHit::Song(song("a", "b")).track().is_some());
        assert!(SearchHit::Video(song("a", "b")).track().is_some());
        assert!(SearchHit::Album(AlbumRef {
            id: "1".into(),
            title: "t".into(),
            year: "2020".into(),
        })
        .track()
        .is_none());
        assert!(SearchHit::Artist(ArtistRef {
            channel_id: "1".into(),
            name: "n".into(),
            subtitle: String::new(),
        })
        .track()
        .is_none());
    }

    #[test]
    fn a_row_always_says_what_kind_it_is() {
        // The list mixes playable and non-playable rows, so the trailing
        // column has to distinguish them.
        assert_eq!(SearchHit::Song(song("a", "b")).trailing(), "3:20");
        assert_eq!(
            SearchHit::Playlist(Playlist {
                id: "1".into(),
                title: "t".into(),
                author: "me".into(),
                track_count: "12 songs".into(),
            })
            .trailing(),
            "playlist"
        );
    }

    #[test]
    fn a_playlist_row_shows_its_author_and_size() {
        let p = SearchHit::Playlist(Playlist {
            id: "1".into(),
            title: "Mix".into(),
            author: "me".into(),
            track_count: "12 songs".into(),
        });
        assert_eq!(p.label(), "Mix");
        assert_eq!(p.sublabel(), "me · 12 songs");
    }
}
