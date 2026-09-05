//! Search, autocomplete, radio, lyrics and ratings -- everything that
//! reaches the API on the user's behalf.

use super::*;

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
        self.searching = true;
        self.search_editing = false;
        self.suggestions.clear();
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match api.search_songs(&q).await {
                Ok(t) => {
                    let _ = tx.send(AppMsg::SearchResults(t));
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::Error(format!("search: {e}")));
                }
            }
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
