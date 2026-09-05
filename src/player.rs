//! mpv playback backend, driven over its JSON IPC socket.
//!
//! We deliberately don't decode audio ourselves. mpv already handles opus/AAC,
//! HTTP streaming, seeking, gapless transitions and replaygain, and it does it
//! in its own process so a slow redraw can never stutter playback. Our job is
//! to keep two tracks in mpv's playlist (current + next) so the next one is
//! already resolved and buffered before the current ends -- that is what makes
//! transitions gapless despite every track needing a yt-dlp round trip.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use time::{Date, Month, OffsetDateTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};

/// Property observation handles. Numbers are arbitrary but must be stable,
/// since mpv echoes them back on every change.
mod obs {
    pub(super) const TIME_POS: i64 = 1;
    pub(super) const DURATION: i64 = 2;
    pub(super) const PAUSE: i64 = 3;
    pub(super) const VOLUME: i64 = 4;
    pub(super) const CORE_IDLE: i64 = 5;
    pub(super) const IDLE_ACTIVE: i64 = 6;
    pub(super) const PLAYLIST_POS: i64 = 7;
    pub(super) const CACHE_WAIT: i64 = 8;
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub paused: bool,
    pub time_pos: f64,
    pub duration: f64,
    pub volume: f64,
    /// mpv is alive but has nothing queued.
    pub idle: bool,
    /// Stalled waiting on the network, as opposed to user-paused.
    pub buffering: bool,
    pub playlist_pos: i64,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            paused: false,
            time_pos: 0.0,
            duration: 0.0,
            volume: 100.0,
            idle: true,
            buffering: false,
            playlist_pos: -1,
        }
    }
}

/// Things mpv tells us that the app needs to react to, rather than just
/// display.
#[derive(Clone, Debug)]
pub enum PlayerEvent {
    /// A file finished. `reason` is mpv's: "eof", "stop", "quit", "error".
    EndFile {
        reason: String,
    },
    FileLoaded,
    /// mpv moved through its internal playlist on its own (gapless advance).
    PlaylistPos(i64),
    Idle,
    /// mpv died. The app should surface this rather than silently hang.
    Exited,
}

pub struct Player {
    tx: mpsc::UnboundedSender<Value>,
    state: Arc<RwLock<PlayerState>>,
    req_id: AtomicI64,
    socket: PathBuf,
    child: Arc<RwLock<Option<Child>>>,
    /// Where the stream extractor was found, for reporting its version when
    /// tracks start failing.
    extractor: PathBuf,
    /// Ceiling for the volume. Above 100 mpv applies plain digital gain with
    /// nothing to catch the peaks, so anything already near full scale clips
    /// and the track turns fuzzy. Boosting stays available for quiet
    /// recordings, but only if the user asks for it in the config.
    volume_max: f64,
}

/// The stream extractors mpv can shell out to, best first.
const EXTRACTORS: [&str; 2] = ["yt-dlp", "youtube-dl"];

/// Locate the stream extractor before mpv needs it.
///
/// mpv resolves a YouTube URL by running this binary, and without it mpv
/// still starts, still accepts commands, and fails every single track with
/// nothing on screen to explain why. That is the worst failure a music
/// player can have, so it is worth catching at startup and saying plainly.
pub fn find_extractor(configured: &str) -> Result<PathBuf> {
    find_extractor_in(configured, |name| which::which(name).ok())
}

/// The search itself, with the lookup passed in so it can be tested without
/// touching the process's environment.
fn find_extractor_in(
    configured: &str,
    lookup: impl Fn(&str) -> Option<PathBuf>,
) -> Result<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        // `which` accepts a path as well as a bare name, and applies the same
        // executable test to both.
        if let Ok(path) = which::which(configured) {
            return Ok(path);
        }
        bail!(
            "ytdlp_path points at {configured:?}, which is not an executable file.\n\
             Fix it in the config, or clear it to search PATH instead."
        );
    }
    for name in EXTRACTORS {
        if let Some(path) = lookup(name) {
            return Ok(path);
        }
    }
    bail!(
        "yt-dlp not found on PATH.\n\n\
         mpv needs it to turn a YouTube video into a playable stream; without \
         it nothing will play.\n\n\
         Install it with one of:\n  \
           pipx install yt-dlp\n  \
           apt install yt-dlp        # or dnf, pacman, zypper\n  \
           brew install yt-dlp\n\n\
         Already have it somewhere else? Put the path in \
         ~/.config/ytkew/config.toml:\n  \
           ytdlp_path = \"/opt/bin/yt-dlp\""
    )
}

/// Ask the extractor what version it is.
///
/// yt-dlp names its releases after the date they were cut, so the version
/// string doubles as an age.
pub fn extractor_version(path: &Path) -> Option<String> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// How many days old a `YYYY.MM.DD` version string is, relative to `today`.
/// `None` if it is not a dated release.
pub fn version_age_days(version: &str, today: Date) -> Option<i64> {
    // Releases can carry a build suffix, as in "2025.03.14.232715".
    let mut parts = version.trim().split('.');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    let released = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    Some((today - released).whole_days())
}

/// Today's date, or the epoch if the clock is unreadable -- which makes every
/// version look ancient rather than crashing over it.
pub fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

/// Past this, a stale extractor is the likeliest explanation for streams
/// failing to resolve. yt-dlp ships roughly monthly and YouTube breaks it
/// more often than that.
pub const STALE_AFTER_DAYS: i64 = 45;

/// What to tell the user after tracks fail to load.
///
/// Distinguishes "your tool is out of date" from "these particular tracks are
/// unavailable", which need completely different responses -- and the first
/// is by far the more common.
pub fn staleness_advice(version: Option<&str>, today: Date) -> String {
    match version.and_then(|v| version_age_days(v, today).map(|a| (v, a))) {
        Some((v, age)) if age > STALE_AFTER_DAYS => format!(
            "tracks keep failing — yt-dlp {v} is {} old. Run `yt-dlp -U`",
            humanise_days(age)
        ),
        Some((v, _)) => {
            format!("tracks keep failing — yt-dlp {v} is current, so they may be unavailable")
        }
        None => "tracks keep failing — try updating yt-dlp (`yt-dlp -U`)".to_string(),
    }
}

fn humanise_days(days: i64) -> String {
    match days {
        d if d >= 730 => format!("{} years", d / 365),
        d if d >= 365 => "over a year".to_string(),
        d if d >= 60 => format!("{} months", d / 30),
        d => format!("{d} days"),
    }
}

impl Player {
    /// Spawn mpv and connect to its IPC socket.
    pub async fn spawn(
        initial_volume: f64,
        volume_max: f64,
        ytdlp_path: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PlayerEvent>)> {
        let volume_max = volume_max.clamp(100.0, 130.0);
        let initial_volume = initial_volume.clamp(0.0, volume_max);
        // Before mpv, not after: a missing extractor is the one failure that
        // otherwise looks like the program working.
        let extractor = find_extractor(ytdlp_path)?;
        let socket = socket_path();
        let _ = std::fs::remove_file(&socket);

        let mut child = Command::new("mpv")
            .arg("--no-video")
            .arg("--no-terminal")
            .arg("--idle=yes")
            .arg("--audio-display=no")
            .arg(format!("--input-ipc-server={}", socket.display()))
            // Prefer opus (what YouTube Music actually serves) and never fall
            // back to a video-bearing stream.
            .arg("--ytdl-format=bestaudio[acodec=opus]/bestaudio/best")
            .arg(format!(
                "--script-opts=ytdl_hook-ytdl_path={}",
                extractor.display()
            ))
            // The two options that buy us gapless across network tracks.
            .arg("--prefetch-playlist=yes")
            .arg("--gapless-audio=yes")
            .arg("--cache=yes")
            .arg("--demuxer-max-bytes=64MiB")
            .arg("--demuxer-readahead-secs=20")
            .arg("--replaygain=track")
            // Let replaygain attenuate rather than clip when a track's tag
            // asks for more headroom than the stream has.
            .arg("--replaygain-clip=no")
            .arg(format!("--volume-max={volume_max}"))
            .arg(format!("--volume={initial_volume}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            // Piped, not discarded: when mpv refuses to start, this is the
            // only place that says why. Drained by a task below so the pipe
            // cannot fill and wedge mpv.
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context(
                "failed to spawn mpv.\n\n\
                 Install it with one of:\n  \
                   apt install mpv          # or dnf, pacman, zypper\n  \
                   brew install mpv",
            )?;

        let complaints = drain_stderr(&mut child);
        let stream = connect_with_retry(&socket, &mut child, &complaints).await?;
        let (read_half, mut write_half) = stream.into_split();

        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<PlayerEvent>();
        let state = Arc::new(RwLock::new(PlayerState {
            volume: initial_volume,
            ..Default::default()
        }));

        // Writer: serialize commands onto the socket.
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                let mut line = cmd.to_string();
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        // Reader: fold property changes into shared state, forward real events.
        let st = state.clone();
        let etx = ev_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                handle_message(&msg, &st, &etx).await;
            }
            let _ = etx.send(PlayerEvent::Exited);
        });

        let player = Self {
            tx,
            state,
            req_id: AtomicI64::new(100),
            socket,
            child: Arc::new(RwLock::new(Some(child))),
            volume_max,
            extractor,
        };
        player.observe_properties()?;
        Ok((player, ev_rx))
    }

    fn observe_properties(&self) -> Result<()> {
        for (id, name) in [
            (obs::TIME_POS, "time-pos"),
            (obs::DURATION, "duration"),
            (obs::PAUSE, "pause"),
            (obs::VOLUME, "volume"),
            (obs::CORE_IDLE, "core-idle"),
            (obs::IDLE_ACTIVE, "idle-active"),
            (obs::PLAYLIST_POS, "playlist-pos"),
            (obs::CACHE_WAIT, "paused-for-cache"),
        ] {
            self.command(json!(["observe_property", id, name]))?;
        }
        Ok(())
    }

    fn command(&self, args: Value) -> Result<()> {
        let id = self.req_id.fetch_add(1, Ordering::Relaxed);
        self.tx
            .send(json!({ "command": args, "request_id": id }))
            .map_err(|_| anyhow!("mpv IPC channel closed"))
    }

    pub async fn state(&self) -> PlayerState {
        self.state.read().await.clone()
    }

    // --- playback control -------------------------------------------------

    /// Replace the playlist with `url` and start playing immediately.
    pub fn play_now(&self, url: &str) -> Result<()> {
        self.command(json!(["loadfile", url, "replace"]))
    }

    /// Append to mpv's playlist so it gets resolved and buffered ahead of time.
    pub fn append(&self, url: &str) -> Result<()> {
        self.command(json!(["loadfile", url, "append"]))
    }

    pub fn clear_playlist(&self) -> Result<()> {
        self.command(json!(["playlist-clear"]))
    }

    pub fn stop(&self) -> Result<()> {
        self.command(json!(["stop"]))
    }

    pub fn set_pause(&self, paused: bool) -> Result<()> {
        self.command(json!(["set_property", "pause", paused]))
    }

    pub async fn toggle_pause(&self) -> Result<()> {
        let paused = self.state.read().await.paused;
        self.set_pause(!paused)
    }

    /// Relative seek, clamped by mpv itself.
    pub fn seek(&self, secs: f64) -> Result<()> {
        self.command(json!(["seek", secs, "relative"]))
    }

    pub fn seek_absolute(&self, secs: f64) -> Result<()> {
        self.command(json!(["seek", secs, "absolute"]))
    }

    pub fn extractor_path(&self) -> &Path {
        &self.extractor
    }

    pub async fn add_volume(&self, delta: f64) -> Result<f64> {
        let cur = self.state.read().await.volume;
        let next = (cur + delta).clamp(0.0, self.volume_max);
        self.command(json!(["set_property", "volume", next]))?;
        self.state.write().await.volume = next;
        Ok(next)
    }

    /// Set volume absolutely. MPRIS hands us a level rather than a delta.
    pub async fn set_volume(&self, vol: f64) -> Result<()> {
        let v = vol.clamp(0.0, self.volume_max);
        self.command(json!(["set_property", "volume", v]))?;
        self.state.write().await.volume = v;
        Ok(())
    }

    /// Ask mpv to skip forward within its own prefetched playlist. Returns
    /// false when there is nothing prefetched, so the caller can fall back to
    /// loading the next queue entry itself.
    pub async fn playlist_next(&self) -> Result<()> {
        self.command(json!(["playlist-next", "force"]))
    }

    /// Let mpv loop the current file itself; cheaper and more precise than
    /// re-resolving the stream on every repeat.
    pub fn set_loop_file(&self, on: bool) -> Result<()> {
        let v = if on { "inf" } else { "no" };
        self.command(json!(["set_property", "loop-file", v]))
    }

    pub async fn shutdown(&self) {
        let _ = self.command(json!(["quit"]));
        // Give mpv a moment to exit cleanly, then make sure it's gone.
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        if let Some(mut child) = self.child.write().await.take() {
            let _ = child.kill().await;
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

async fn handle_message(
    msg: &Value,
    state: &Arc<RwLock<PlayerState>>,
    ev: &mpsc::UnboundedSender<PlayerEvent>,
) {
    let Some(event) = msg.get("event").and_then(Value::as_str) else {
        return;
    };
    match event {
        "property-change" => {
            let id = msg.get("id").and_then(Value::as_i64).unwrap_or(-1);
            let data = msg.get("data");
            let mut s = state.write().await;
            match id {
                obs::TIME_POS => {
                    if let Some(v) = data.and_then(Value::as_f64) {
                        s.time_pos = v;
                    }
                }
                obs::DURATION => {
                    // Null duration is normal while a stream is resolving.
                    s.duration = data.and_then(Value::as_f64).unwrap_or(0.0);
                }
                obs::PAUSE => {
                    if let Some(v) = data.and_then(Value::as_bool) {
                        s.paused = v;
                    }
                }
                obs::VOLUME => {
                    if let Some(v) = data.and_then(Value::as_f64) {
                        s.volume = v;
                    }
                }
                obs::IDLE_ACTIVE => {
                    if let Some(v) = data.and_then(Value::as_bool) {
                        s.idle = v;
                        if v {
                            let _ = ev.send(PlayerEvent::Idle);
                        }
                    }
                }
                obs::CACHE_WAIT => {
                    s.buffering = data.and_then(Value::as_bool).unwrap_or(false);
                }
                obs::PLAYLIST_POS => {
                    let pos = data.and_then(Value::as_i64).unwrap_or(-1);
                    if pos != s.playlist_pos {
                        s.playlist_pos = pos;
                        let _ = ev.send(PlayerEvent::PlaylistPos(pos));
                    }
                }
                _ => {}
            }
        }
        "end-file" => {
            let reason = msg
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let _ = ev.send(PlayerEvent::EndFile { reason });
        }
        "file-loaded" => {
            let _ = ev.send(PlayerEvent::FileLoaded);
        }
        _ => {}
    }
}

fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("ytkew-{}.sock", std::process::id()))
}

/// mpv creates the socket a little after exec, so poll briefly for it.
/// How long to let mpv get as far as creating its socket.
///
/// Generous on purpose. A cold start on macOS can take seconds the first
/// time -- Gatekeeper verifies the binary and every dylib Homebrew pulled in
/// behind it -- and the old five-second budget turned that into a failure
/// that went away when you ran it again. Waiting longer costs nothing when
/// mpv is healthy, because the moment the socket answers we stop; and it
/// costs nothing when mpv is broken either, because a dead child is noticed
/// immediately rather than waited out.
const SOCKET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Collect mpv's stderr in the background, keeping the tail.
///
/// Must be drained rather than merely piped: a pipe nobody reads fills up and
/// blocks the writer, and a wedged mpv is worse than a noisy one.
fn drain_stderr(child: &mut Child) -> Arc<std::sync::Mutex<Vec<String>>> {
    let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
    let Some(stderr) = child.stderr.take() else {
        return lines;
    };
    let sink = lines.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let mut held = sink.lock().unwrap_or_else(|e| e.into_inner());
            // Only the tail matters, and mpv can be chatty about codecs.
            if held.len() == 20 {
                held.remove(0);
            }
            held.push(line);
        }
    });
    lines
}

fn complaints_of(lines: &Arc<std::sync::Mutex<Vec<String>>>) -> String {
    let held = lines.lock().unwrap_or_else(|e| e.into_inner());
    if held.is_empty() {
        return String::new();
    }
    format!("\n\nmpv said:\n  {}", held.join("\n  "))
}

async fn connect_with_retry(
    path: &PathBuf,
    child: &mut Child,
    complaints: &Arc<std::sync::Mutex<Vec<String>>>,
) -> Result<UnixStream> {
    let start = std::time::Instant::now();
    while start.elapsed() < SOCKET_TIMEOUT {
        if path.exists() {
            if let Ok(s) = UnixStream::connect(path).await {
                return Ok(s);
            }
        }
        // A child that has already exited is never going to create the
        // socket, so say so now with whatever mpv managed to complain about
        // rather than sitting out the whole timeout.
        if let Ok(Some(status)) = child.try_wait() {
            // Give the reader a moment to catch the last of stderr.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            bail!(
                "mpv exited immediately ({status}).{}\n\n\
                 ytkew needs mpv 0.30 or newer. Check yours with `mpv --version`.",
                complaints_of(complaints)
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    bail!(
        "mpv did not open its IPC socket at {} within {}s.{}",
        path.display(),
        SOCKET_TIMEOUT.as_secs(),
        complaints_of(complaints)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ytkew-ext-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    fn fake_binary(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// A stand-in for `which`, so the tests never depend on what happens to
    /// be installed on the machine running them.
    fn only(available: &[(&'static str, PathBuf)]) -> impl Fn(&str) -> Option<PathBuf> {
        let owned: Vec<_> = available.to_vec();
        move |name| {
            owned
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, p)| p.clone())
        }
    }

    #[test]
    fn a_configured_path_is_used_as_given() {
        let dir = scratch("configured");
        let bin = fake_binary(&dir, "my-ytdlp");
        let found = find_extractor_in(bin.to_str().unwrap(), only(&[])).unwrap();
        assert_eq!(found, bin);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_configured_path_that_is_wrong_says_so_rather_than_falling_back() {
        // Silently searching PATH would hide the typo and leave the user
        // wondering why their setting had no effect.
        let dir = scratch("wrong");
        let good = fake_binary(&dir, "yt-dlp");
        let missing = dir.join("not-here");
        let e = find_extractor_in(missing.to_str().unwrap(), only(&[("yt-dlp", good)]))
            .unwrap_err()
            .to_string();
        assert!(e.contains("ytdlp_path"), "got {e}");
        assert!(e.contains("not an executable file"), "got {e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn yt_dlp_is_preferred_over_youtube_dl() {
        let dir = scratch("both");
        let a = fake_binary(&dir, "yt-dlp");
        let b = fake_binary(&dir, "youtube-dl");
        let found = find_extractor_in("", only(&[("yt-dlp", a), ("youtube-dl", b)])).unwrap();
        assert_eq!(found.file_name().unwrap(), "yt-dlp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn youtube_dl_is_accepted_when_yt_dlp_is_absent() {
        let dir = scratch("fallback");
        let b = fake_binary(&dir, "youtube-dl");
        let found = find_extractor_in("", only(&[("youtube-dl", b)])).unwrap();
        assert_eq!(found.file_name().unwrap(), "youtube-dl");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_error_tells_you_how_to_fix_it() {
        // This message is the whole point of the check: without it a missing
        // extractor is a player that runs perfectly and never makes a sound.
        let e = find_extractor_in("", only(&[])).unwrap_err().to_string();
        assert!(e.contains("yt-dlp not found"), "got {e}");
        assert!(e.contains("install yt-dlp"), "got {e}");
        assert!(e.contains("ytdlp_path"), "got {e}");
    }
}
