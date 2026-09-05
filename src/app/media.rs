//! MPRIS glue: publishing transport state to D-Bus and applying what
//! comes back.

use crate::config::Action;
use crate::mpris::{metadata_for, MprisCommand, MprisState};
use crate::queue::RepeatMode;
use anyhow::Result;

use super::*;

impl App {
    /// Current transport state as MPRIS describes it.
    pub fn mpris_snapshot(&self) -> MprisState {
        use mpris_server::{LoopStatus, PlaybackStatus};
        let track = self.queue.current();
        // Point clients at the cached file when it exists; most only handle
        // file:// for artwork.
        let art = track.and_then(|t| t.thumbnail.as_ref()).and_then(|url| {
            let p = self.covers.cache_path(url);
            p.exists().then_some(p)
        });
        let status = match track {
            None => PlaybackStatus::Stopped,
            Some(_) if self.player_state.paused => PlaybackStatus::Paused,
            Some(_) => PlaybackStatus::Playing,
        };
        MprisState {
            status,
            metadata: track
                .map(|t| metadata_for(t, art.as_deref()))
                .unwrap_or_default(),
            // The art path is part of the key so metadata is republished once
            // the cover finishes downloading, not just when the track changes.
            track_key: track
                .map(|t| {
                    format!(
                        "{}|{}",
                        t.video_id,
                        art.as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    )
                })
                .unwrap_or_default(),
            position: mpris_server::Time::from_micros(
                (self.player_state.time_pos.max(0.0) * 1e6) as i64,
            ),
            // MPRIS volume is a 0..1 fraction; ours is a percentage.
            volume: (self.player_state.volume / 100.0).clamp(0.0, 1.0),
            can_next: self.queue.peek_next().is_some(),
            can_prev: track.is_some(),
            can_play: track.is_some(),
            can_seek: track.is_some(),
            shuffle: self.queue.shuffle,
            loop_status: match self.queue.repeat {
                RepeatMode::Off => LoopStatus::None,
                RepeatMode::All => LoopStatus::Playlist,
                RepeatMode::One => LoopStatus::Track,
            },
        }
    }

    /// Tell D-Bus clients the position jumped. Without this their progress
    /// bars keep counting from where they last polled and drift out of step
    /// after every seek.
    pub(crate) fn announce_seek(&mut self, secs: f64) {
        self.pending_seek = Some(secs.max(0.0));
    }

    /// Push current state onto D-Bus. Cheap when nothing changed.
    pub async fn sync_mpris(&mut self) {
        let Some(m) = &self.mpris else { return };
        // Position is polled by clients, so update it without a signal.
        m.set_position(self.player_state.time_pos);
        m.publish(self.mpris_snapshot()).await;
        if let Some(secs) = self.pending_seek.take() {
            m.seeked(secs).await;
        }
    }

    /// Apply a request that arrived over D-Bus (a media key, usually).
    pub async fn handle_mpris(&mut self, cmd: MprisCommand) -> Result<()> {
        use mpris_server::LoopStatus;
        match cmd {
            MprisCommand::PlayPause => self.handle_action(Action::PlayPause).await?,
            MprisCommand::Play => self.player.set_pause(false)?,
            MprisCommand::Pause => self.player.set_pause(true)?,
            MprisCommand::Stop => self.handle_action(Action::Stop).await?,
            MprisCommand::Next => self.next_track().await,
            MprisCommand::Prev => self.prev_track(),
            MprisCommand::Seek(secs) => self.player.seek(secs)?,
            MprisCommand::SetPosition(secs) => {
                self.player.seek_absolute(secs)?;
                self.announce_seek(secs);
            }
            MprisCommand::SetVolume(fraction) => {
                self.player.set_volume(fraction * 100.0).await?;
            }
            MprisCommand::SetShuffle(on) => {
                if self.queue.shuffle != on {
                    self.queue.toggle_shuffle();
                    self.resync_prefetch();
                }
            }
            MprisCommand::SetLoop(status) => {
                self.queue.repeat = match status {
                    LoopStatus::None => RepeatMode::Off,
                    LoopStatus::Playlist => RepeatMode::All,
                    LoopStatus::Track => RepeatMode::One,
                };
                let _ = self
                    .player
                    .set_loop_file(self.queue.repeat == RepeatMode::One);
                self.resync_prefetch();
            }
            MprisCommand::Quit => self.should_quit = true,
            // We do not own the terminal window, so there is nothing to raise.
            MprisCommand::Raise => {}
        }
        Ok(())
    }
}
