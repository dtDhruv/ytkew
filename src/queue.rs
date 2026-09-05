//! Playback queue.
//!
//! `tracks` holds the queue in the order the user sees and edits it; `order`
//! is the order playback actually follows. Keeping them separate means
//! toggling shuffle never scrambles the list on screen, and toggling it back
//! off restores the original sequence rather than an approximation of it.

use crate::model::Track;
use rand::seq::SliceRandom;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    /// Loop the current track forever.
    One,
    /// Wrap around to the start when the queue ends.
    All,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

}

#[derive(Default)]
pub struct Queue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    /// Index into `order`, not into `tracks`.
    pos: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Position of the current track within `tracks` (what the UI highlights).
    pub fn current_index(&self) -> Option<usize> {
        self.pos.and_then(|p| self.order.get(p).copied())
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index().and_then(|i| self.tracks.get(i))
    }

    /// 1-based position for the "(3/12)" status readout.
    pub fn human_position(&self) -> (usize, usize) {
        (self.pos.map(|p| p + 1).unwrap_or(0), self.tracks.len())
    }

    pub fn push(&mut self, track: Track) {
        let idx = self.tracks.len();
        self.tracks.push(track);
        // A newly appended track goes at the end of the playback order even
        // when shuffled, so "add to queue" stays predictable mid-listen.
        self.order.push(idx);
    }

    pub fn extend(&mut self, tracks: impl IntoIterator<Item = Track>) {
        for t in tracks {
            self.push(t);
        }
    }

    /// Replace the whole queue and start from `start`.
    pub fn replace(&mut self, tracks: Vec<Track>, start: usize) {
        self.tracks = tracks;
        self.order = (0..self.tracks.len()).collect();
        if self.shuffle {
            self.reshuffle_keeping(Some(start));
        } else {
            self.pos = if self.tracks.is_empty() {
                None
            } else {
                Some(start.min(self.tracks.len() - 1))
            };
        }
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.order.clear();
        self.pos = None;
    }

    /// Set the current track by its index in `tracks`.
    pub fn jump_to(&mut self, track_index: usize) -> Option<&Track> {
        let p = self.order.iter().position(|&i| i == track_index)?;
        self.pos = Some(p);
        self.current()
    }

    /// The track that would play next, without advancing. Used both for
    /// prefetch and to decide whether to stop at the end of the queue.
    pub fn peek_next(&self) -> Option<&Track> {
        self.next_pos().and_then(|p| {
            self.order.get(p).and_then(|&i| self.tracks.get(i))
        })
    }

    fn next_pos(&self) -> Option<usize> {
        let pos = self.pos?;
        if self.tracks.is_empty() {
            return None;
        }
        match self.repeat {
            // Repeat-one is handled by the caller (it re-seeks rather than
            // re-resolving the stream), so for ordering purposes it advances.
            RepeatMode::One | RepeatMode::Off => {
                if pos + 1 < self.order.len() {
                    Some(pos + 1)
                } else {
                    None
                }
            }
            RepeatMode::All => Some((pos + 1) % self.order.len()),
        }
    }

    pub fn advance(&mut self) -> Option<&Track> {
        let p = self.next_pos()?;
        self.pos = Some(p);
        self.current()
    }

    pub fn previous(&mut self) -> Option<&Track> {
        let pos = self.pos?;
        let p = if pos > 0 {
            pos - 1
        } else if self.repeat == RepeatMode::All && !self.order.is_empty() {
            self.order.len() - 1
        } else {
            0
        };
        self.pos = Some(p);
        self.current()
    }

    pub fn toggle_shuffle(&mut self) -> bool {
        self.shuffle = !self.shuffle;
        let current = self.current_index();
        if self.shuffle {
            self.reshuffle_keeping(current);
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.pos = current;
        }
        self.shuffle
    }

    /// Shuffle the playback order, pinning `keep` to the front so the track
    /// you are currently hearing does not restart.
    fn reshuffle_keeping(&mut self, keep: Option<usize>) {
        let mut rest: Vec<usize> = (0..self.tracks.len())
            .filter(|i| Some(*i) != keep)
            .collect();
        rest.shuffle(&mut rand::rng());
        self.order = match keep {
            Some(k) => std::iter::once(k).chain(rest).collect(),
            None => rest,
        };
        self.pos = if self.order.is_empty() { None } else { Some(0) };
    }

    pub fn remove(&mut self, track_index: usize) {
        if track_index >= self.tracks.len() {
            return;
        }
        let current = self.current_index();
        self.tracks.remove(track_index);
        // Rebuild the order, dropping the removed index and shifting the rest.
        self.order.retain(|&i| i != track_index);
        for i in self.order.iter_mut() {
            if *i > track_index {
                *i -= 1;
            }
        }
        // Keep pointing at the same track where possible.
        self.pos = match current {
            Some(c) if c == track_index => {
                // The playing track went away; stay at the same slot.
                let p = self.pos.unwrap_or(0);
                if self.order.is_empty() {
                    None
                } else {
                    Some(p.min(self.order.len() - 1))
                }
            }
            Some(c) => {
                let adjusted = if c > track_index { c - 1 } else { c };
                self.order.iter().position(|&i| i == adjusted)
            }
            None => None,
        };
    }

    /// Move a track up in the *visible* queue. Only meaningful unshuffled.
    pub fn move_up(&mut self, track_index: usize) -> Option<usize> {
        if track_index == 0 || track_index >= self.tracks.len() {
            return None;
        }
        self.swap_visible(track_index, track_index - 1);
        Some(track_index - 1)
    }

    pub fn move_down(&mut self, track_index: usize) -> Option<usize> {
        if track_index + 1 >= self.tracks.len() {
            return None;
        }
        self.swap_visible(track_index, track_index + 1);
        Some(track_index + 1)
    }

    fn swap_visible(&mut self, a: usize, b: usize) {
        self.tracks.swap(a, b);
        // Order holds indices into tracks, so swapping rows means relabeling.
        for i in self.order.iter_mut() {
            if *i == a {
                *i = b;
            } else if *i == b {
                *i = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize) -> Queue {
        let mut q = Queue::new();
        q.extend((0..n).map(|i| Track {
            video_id: format!("v{i}"),
            title: format!("t{i}"),
            ..Default::default()
        }));
        q.pos = Some(0);
        q
    }

    #[test]
    fn advances_and_stops_at_end() {
        let mut q = q(3);
        assert_eq!(q.current().unwrap().title, "t0");
        assert_eq!(q.advance().unwrap().title, "t1");
        assert_eq!(q.advance().unwrap().title, "t2");
        assert!(q.advance().is_none(), "should stop at end with repeat off");
    }

    #[test]
    fn repeat_all_wraps() {
        let mut q = q(2);
        q.repeat = RepeatMode::All;
        q.advance();
        assert_eq!(q.advance().unwrap().title, "t0");
    }

    #[test]
    fn shuffle_keeps_current_track_playing() {
        let mut q = q(20);
        q.jump_to(7);
        let before = q.current().unwrap().video_id.clone();
        q.toggle_shuffle();
        assert_eq!(q.current().unwrap().video_id, before);
        // And the visible list is untouched.
        assert_eq!(q.tracks()[7].video_id, "v7");
    }

    #[test]
    fn unshuffle_restores_original_order() {
        let mut q = q(10);
        q.toggle_shuffle();
        q.toggle_shuffle();
        assert_eq!(q.order, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn removing_shifts_indices() {
        let mut q = q(4);
        q.jump_to(3);
        q.remove(1);
        assert_eq!(q.len(), 3);
        assert_eq!(q.current().unwrap().video_id, "v3");
    }

    #[test]
    fn moving_a_row_preserves_current_track() {
        let mut q = q(4);
        q.jump_to(2);
        q.move_up(2);
        assert_eq!(q.tracks()[1].video_id, "v2");
        assert_eq!(q.current().unwrap().video_id, "v2");
    }
}
