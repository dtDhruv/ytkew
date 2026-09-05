//! MPRIS (org.mpris.MediaPlayer2) over D-Bus.
//!
//! This is what makes media keys work and what puts ytkew in the GNOME/KDE
//! now-playing panel: the desktop sends us method calls, and we advertise
//! state through properties.
//!
//! We implement `RootInterface`/`PlayerInterface` by hand rather than using
//! the crate's convenience `Player` type, because that one is `Rc`-based and
//! needs `spawn_local`, which does not fit a multi-threaded tokio runtime.
//! `Server<T>` requires `T: Send + Sync`, so this shape is spawnable.

use anyhow::Result;
use mpris_server::zbus::{fdo, Result as ZResult};
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Signal, Time, TrackId, Volume,
};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// A request from the desktop environment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MprisCommand {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    /// Relative seek, in seconds.
    Seek(f64),
    /// Absolute position, in seconds.
    SetPosition(f64),
    /// 0.0..=1.0, as MPRIS defines volume.
    SetVolume(f64),
    SetShuffle(bool),
    SetLoop(LoopStatus),
    Quit,
    Raise,
}

/// The state the D-Bus interface serves to clients.
#[derive(Clone)]
pub struct MprisState {
    pub status: PlaybackStatus,
    pub metadata: Metadata,
    /// Identity of the current track, so metadata changes are cheap to detect
    /// without requiring `Metadata: PartialEq`.
    pub track_key: String,
    pub position: Time,
    /// 0.0..=1.0.
    pub volume: f64,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_play: bool,
    pub can_seek: bool,
    pub shuffle: bool,
    pub loop_status: LoopStatus,
}

impl Default for MprisState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            metadata: Metadata::new(),
            track_key: String::new(),
            position: Time::from_micros(0),
            volume: 1.0,
            can_next: false,
            can_prev: false,
            can_play: false,
            can_seek: false,
            shuffle: false,
            loop_status: LoopStatus::None,
        }
    }
}

struct Imp {
    state: Arc<RwLock<MprisState>>,
    tx: mpsc::UnboundedSender<MprisCommand>,
}

impl Imp {
    fn send(&self, cmd: MprisCommand) -> fdo::Result<()> {
        // A closed channel means the app is shutting down; report success
        // rather than an error the desktop would surface to the user.
        let _ = self.tx.send(cmd);
        Ok(())
    }

    /// Read a field. Never held across an await, which is what keeps these
    /// futures `Send`.
    fn get<T>(&self, f: impl FnOnce(&MprisState) -> T) -> T {
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }
}

impl RootInterface for Imp {
    async fn raise(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Raise)
    }

    async fn quit(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Quit)
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _fullscreen: bool) -> ZResult<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        // We cannot focus a terminal window we do not own.
        Ok(false)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok("ytkew".into())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok("ytkew".into())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        // OpenUri is refused, so claiming a scheme here would invite calls
        // that can only fail.
        Ok(vec![])
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
}

impl PlayerInterface for Imp {
    async fn next(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Next)
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Prev)
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Pause)
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.send(MprisCommand::PlayPause)
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Stop)
    }

    async fn play(&self) -> fdo::Result<()> {
        self.send(MprisCommand::Play)
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        self.send(MprisCommand::Seek(offset.as_micros() as f64 / 1e6))
    }

    async fn set_position(&self, _track_id: TrackId, position: Time) -> fdo::Result<()> {
        self.send(MprisCommand::SetPosition(
            position.as_micros() as f64 / 1e6,
        ))
    }

    async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("ytkew cannot open URIs".into()))
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(self.get(|s| s.status))
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(self.get(|s| s.loop_status))
    }

    async fn set_loop_status(&self, loop_status: LoopStatus) -> ZResult<()> {
        let _ = self.tx.send(MprisCommand::SetLoop(loop_status));
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _rate: PlaybackRate) -> ZResult<()> {
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.shuffle))
    }

    async fn set_shuffle(&self, shuffle: bool) -> ZResult<()> {
        let _ = self.tx.send(MprisCommand::SetShuffle(shuffle));
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(self.get(|s| s.metadata.clone()))
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.get(|s| s.volume))
    }

    async fn set_volume(&self, volume: Volume) -> ZResult<()> {
        let _ = self.tx.send(MprisCommand::SetVolume(volume));
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(self.get(|s| s.position))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.can_next))
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.can_prev))
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.can_play))
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.can_play))
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(self.get(|s| s.can_seek))
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

pub struct Mpris {
    server: Server<Imp>,
    state: Arc<RwLock<MprisState>>,
}

impl Mpris {
    /// Claim a bus name and start serving. Returns the command stream the app
    /// should poll.
    pub async fn start() -> Result<(Self, mpsc::UnboundedReceiver<MprisCommand>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(RwLock::new(MprisState::default()));
        let imp = Imp {
            state: state.clone(),
            tx,
        };
        // Suffix only; the crate prepends org.mpris.MediaPlayer2.
        let server = Server::new("ytkew", imp).await?;
        Ok((Self { server, state }, rx))
    }

    /// Update the served position without any D-Bus traffic.
    ///
    /// MPRIS clients poll `Position` rather than expecting a signal for it, so
    /// emitting one every frame would be pure noise on the bus.
    pub fn set_position(&self, secs: f64) {
        if let Ok(mut s) = self.state.write() {
            s.position = Time::from_micros((secs.max(0.0) * 1e6) as i64);
        }
    }

    /// Publish anything that actually changed, emitting `PropertiesChanged`
    /// only for those properties.
    pub async fn publish(&self, next: MprisState) {
        let mut changed: Vec<Property> = Vec::new();
        {
            let Ok(cur) = self.state.read() else { return };
            if cur.status != next.status {
                changed.push(Property::PlaybackStatus(next.status));
            }
            if cur.track_key != next.track_key {
                changed.push(Property::Metadata(next.metadata.clone()));
            }
            if (cur.volume - next.volume).abs() > f64::EPSILON {
                changed.push(Property::Volume(next.volume));
            }
            if cur.can_next != next.can_next {
                changed.push(Property::CanGoNext(next.can_next));
            }
            if cur.can_prev != next.can_prev {
                changed.push(Property::CanGoPrevious(next.can_prev));
            }
            if cur.can_play != next.can_play {
                changed.push(Property::CanPlay(next.can_play));
                changed.push(Property::CanPause(next.can_play));
            }
            if cur.can_seek != next.can_seek {
                changed.push(Property::CanSeek(next.can_seek));
            }
            if cur.shuffle != next.shuffle {
                changed.push(Property::Shuffle(next.shuffle));
            }
            if cur.loop_status != next.loop_status {
                changed.push(Property::LoopStatus(next.loop_status));
            }
        }
        if changed.is_empty() {
            return;
        }
        // Keep the position we already have; `next` carries a stale one.
        let position = self
            .state
            .read()
            .map(|s| s.position)
            .unwrap_or(Time::from_micros(0));
        if let Ok(mut s) = self.state.write() {
            *s = MprisState { position, ..next };
        }
        let _ = self.server.properties_changed(changed).await;
    }

    /// Tell clients the position jumped, so their progress bars resync.
    pub async fn seeked(&self, secs: f64) {
        let position = Time::from_micros((secs.max(0.0) * 1e6) as i64);
        let _ = self.server.emit(Signal::Seeked { position }).await;
    }
}

/// A D-Bus object path segment for a video id.
///
/// Object paths allow only `[A-Za-z0-9_]`, while video ids are base64url and
/// so contain `-`. Hex-encoding keeps the id stable and collision-free; using
/// the queue index instead would change a track's identity whenever the queue
/// was reordered, which clients treat as a different track.
fn track_path(video_id: &str) -> String {
    let mut out = String::from("/org/mpris/MediaPlayer2/ytkew/track/x");
    for b in video_id.as_bytes() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Build MPRIS metadata for a track.
///
/// `art` is a path to the cached cover. Desktop widgets fetch `mpris:artUrl`
/// themselves and most only handle `file://`, so a remote URL would leave
/// them showing no artwork at all.
pub fn metadata_for(track: &crate::model::Track, art: Option<&std::path::Path>) -> Metadata {
    let mut m = Metadata::new();
    if let Ok(id) = TrackId::try_from(track_path(&track.video_id)) {
        m.set_trackid(Some(id));
    }
    m.set_title(Some(track.title.clone()));
    if !track.artist.is_empty() {
        m.set_artist(Some(vec![track.artist.clone()]));
    }
    if let Some(album) = &track.album {
        m.set_album(Some(album.clone()));
    }
    if let Some(d) = track.duration {
        m.set_length(Some(Time::from_micros((d * 1e6) as i64)));
    }
    match art {
        Some(path) => m.set_art_url(Some(format!("file://{}", path.display()))),
        // Fall back to the remote URL; better than nothing for clients that
        // can fetch it.
        None => {
            if let Some(url) = &track.thumbnail {
                m.set_art_url(Some(url.clone()));
            }
        }
    }
    m.set_url(Some(track.url()));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;

    fn track() -> Track {
        Track {
            video_id: "abc-123_x".into(),
            title: "Creep".into(),
            artist: "Radiohead".into(),
            album: Some("Pablo Honey".into()),
            duration: Some(238.0),
            duration_text: "3:58".into(),
            thumbnail: Some("https://example.com/art.jpg".into()),
        }
    }

    #[test]
    fn metadata_carries_the_fields_desktops_display() {
        let m = metadata_for(&track(), None);
        assert_eq!(m.title(), Some("Creep"));
        assert_eq!(m.artist(), Some(vec!["Radiohead".to_string()]));
        assert_eq!(m.album(), Some("Pablo Honey"));
        assert_eq!(m.length(), Some(Time::from_micros(238_000_000)));
        assert_eq!(m.art_url().as_deref(), Some("https://example.com/art.jpg"));
    }

    #[test]
    fn track_id_is_a_valid_object_path_even_for_awkward_video_ids() {
        // "abc-123_x" contains characters object paths forbid.
        let m = metadata_for(&track(), None);
        let id = m.trackid().expect("track id should be set");
        let tail = id.as_str().rsplit('/').next().unwrap();
        assert!(
            tail.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "illegal object path segment: {tail}"
        );
    }

    #[test]
    fn track_ids_follow_the_track_not_its_queue_position() {
        // Reordering the queue must not change a track's identity, or clients
        // treat it as a different song.
        let a = metadata_for(&track(), None).trackid().unwrap();
        let b = metadata_for(&track(), None).trackid().unwrap();
        assert_eq!(a.as_str(), b.as_str());

        let other = Track {
            video_id: "different".into(),
            ..track()
        };
        assert_ne!(
            metadata_for(&other, None).trackid().unwrap().as_str(),
            a.as_str()
        );
    }

    #[test]
    fn a_cached_cover_is_offered_as_a_file_url() {
        let path = std::path::Path::new("/home/u/.cache/ytkew/abc123");
        let m = metadata_for(&track(), Some(path));
        assert_eq!(
            m.art_url().as_deref(),
            Some("file:///home/u/.cache/ytkew/abc123")
        );
    }

    #[test]
    fn a_track_without_optional_fields_still_produces_metadata() {
        let bare = Track {
            video_id: "x".into(),
            title: "Untitled".into(),
            ..Default::default()
        };
        let m = metadata_for(&bare, None);
        assert_eq!(m.title(), Some("Untitled"));
        assert_eq!(m.album(), None);
        assert_eq!(m.length(), None);
    }

    #[test]
    fn default_state_reports_stopped_and_no_capabilities() {
        let s = MprisState::default();
        assert_eq!(s.status, PlaybackStatus::Stopped);
        assert!(!s.can_next && !s.can_prev && !s.can_play);
        assert_eq!(s.loop_status, LoopStatus::None);
    }
}
