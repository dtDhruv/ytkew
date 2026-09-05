//! Application state and the pieces that drive it.
//!
//! [`App`] is the single owner of everything the interface reads. Work that
//! touches the network runs in spawned tasks and reports back through
//! [`AppMsg`], so the UI thread never blocks on the API.
//!
//! The parts live in focused submodules: [`input`] maps keys and mouse events
//! onto actions, [`library`] holds the lazily-loaded tree, [`menu`] the
//! overlay and its settings, [`graphics`] the album-art protocols, [`media`]
//! the D-Bus surface and [`search`] everything that reaches the API.

mod graphics;
mod input;
mod library;
mod media;
mod menu;
mod search;

pub(crate) use graphics::theme_palette;
pub use graphics::Graphics;
pub use input::HitRegions;
pub use library::{LibKind, LibNode, LibRow};
pub(crate) use menu::MenuOutcome;
pub use menu::{MenuScreen, MENU_ITEMS, SETTINGS};

use crate::api::Api;
use crate::art::{Cover, CoverLoader};
use crate::config::{Config, Keymap};
use crate::model::Track;
use crate::mpris::Mpris;
use crate::palette::Palette;
use crate::player::{Player, PlayerEvent, PlayerState};
use crate::queue::{Queue, RepeatMode};
use crate::ui::View;
use crate::visual::Visualizer;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Results of background work, delivered back to the single-threaded UI.
pub enum AppMsg {
    SearchResults(Vec<Track>),
    /// One page of children for the node at `path`. Addressed by path so a
    /// slow response cannot land on the wrong node after the user navigates
    /// away.
    LibChildren {
        path: Vec<usize>,
        children: Vec<LibKind>,
        /// The first page replaces the node's children; later pages append.
        first: bool,
        /// The fetch is finished. Always arrives, even when no page did, so a
        /// node can never be left spinning.
        last: bool,
    },
    LibFailed {
        path: Vec<usize>,
        error: String,
    },
    /// A radio mix to append behind a single played track.
    RadioTail {
        after: String,
        tracks: Vec<Track>,
    },
    /// Autocomplete for the search box, tagged with the query it answers so
    /// a slow response cannot overwrite newer typing.
    Suggestions {
        query: String,
        items: Vec<String>,
    },
    Cover {
        video_id: String,
        cover: Box<Cover>,
    },
    Lyrics {
        video_id: String,
        text: String,
    },
    Error(String),
}

pub struct App {
    pub cfg: Config,
    pub keymap: Keymap,
    pub api: Arc<Api>,
    pub player: Player,
    pub player_state: PlayerState,
    pub queue: Queue,
    pub visual: Visualizer,
    pub covers: Arc<CoverLoader>,

    pub view: View,
    /// Where Help and Lyrics were opened from, so escape returns there
    /// instead of stacking the menu on top of them.
    prev_view: View,
    pub palette: Palette,
    pub cover: Option<Cover>,
    /// video_id the current cover belongs to, so stale loads are ignored.
    pub cover_for: Option<String>,

    pub queue_sel: usize,
    pub library: Vec<LibNode>,
    pub library_sel: usize,
    pub library_loading: bool,
    /// Flattened view of `library`, rebuilt only when the tree changes.
    pub library_rows: Vec<LibRow>,

    pub search_input: String,
    pub search_editing: bool,
    pub search_results: Vec<Track>,
    pub suggestions: Vec<String>,
    pub search_sel: usize,
    pub searching: bool,

    pub lyrics: Option<String>,
    pub lyrics_for: Option<String>,
    pub lyrics_scroll: u16,

    pub status: Option<(String, Instant)>,
    pub should_quit: bool,

    /// Terminal cell size in pixels, for sizing sixel output.
    pub cell_px: (u16, u16),
    /// How `cell_px` was obtained, which decides whether auto mode trusts it.
    pub cell_source: crate::art::terminal::CellSource,

    /// D-Bus presence. None when there is no session bus, e.g. over plain ssh
    /// -- media keys are a bonus, never a requirement.
    pub mpris: Option<Mpris>,

    /// Clickable regions from the last frame.
    pub hits: HitRegions,
    /// A seek waiting to be announced on D-Bus at the next sync.
    pending_seek: Option<f64>,

    /// Whether album art is showing. `b` toggles this; the renderer is a
    /// separate setting, so one key does one thing.
    pub cover_visible: bool,
    /// Active theme name. "cover" means take the colours from the artwork.
    pub theme: String,

    /// btop-style overlay menu: escape opens it, q still quits outright.
    pub menu_open: bool,
    pub menu_sel: usize,
    pub menu_screen: MenuScreen,
    pub option_sel: usize,
    /// A library node whose children were requested so its whole contents
    /// could be played once they arrive.
    pending_play: Option<Vec<usize>>,
    /// Node whose still-arriving pages should extend the queue, set when a
    /// "play all" starts on the first page of a multi-page fetch.
    queue_feed: Option<Vec<usize>>,
    /// Which graphics protocol to draw the cover with, if any.
    graphics: Graphics,
    /// A region just blanked, which the next frame must treat as empty so
    /// ratatui's model matches the screen.
    stale_cover: Option<ratatui::layout::Rect>,
    /// What image is currently on screen: (video id, rect, protocol). The
    /// protocol is recorded because removing a kitty placement needs an
    /// explicit delete, and by the time we remove it the mode may already
    /// have been cycled to something else.
    art_on_screen: Option<(String, ratatui::layout::Rect, Graphics)>,

    /// Last playlist position mpv reported, to detect its own advances.
    mpv_pos: i64,
    tx: mpsc::UnboundedSender<AppMsg>,
}

impl App {
    pub fn new(
        cfg: Config,
        state: crate::config::State,
        api: Arc<Api>,
        player: Player,
        covers: Arc<CoverLoader>,
        tx: mpsc::UnboundedSender<AppMsg>,
    ) -> Self {
        let mut queue = Queue::new();
        // Restore transport state only if the user asked us to remember it.
        if cfg.save_repeat_shuffle {
            queue.shuffle = state.shuffle;
            queue.repeat = match state.repeat.as_str() {
                "all" => RepeatMode::All,
                "one" => RepeatMode::One,
                _ => RepeatMode::Off,
            };
        }
        let cfg_accent = cfg.accent_color;
        // First run falls back to the config's default; after that the
        // remembered toggle wins.
        let state_cover_visible = state.cover_visible && cfg.cover_enabled;
        // A theme picked at runtime outranks the config.
        let active_theme = if state.theme.is_empty() {
            cfg.theme.clone()
        } else {
            state.theme.clone()
        };
        let start_palette = theme_palette(&active_theme, &cfg, cfg_accent);
        // Resolve the cell size before the TUI starts, since the CSI query
        // needs a quiet terminal.
        let (cfg_cell_px, cell_source) =
            crate::art::terminal::detect_cell_size(match cfg.cell_px {
                [w, h] if w > 0 && h > 0 => Some((w, h)),
                _ => None,
            });
        let cfg_graphics = Graphics::resolve(&cfg, cell_source);
        Self {
            player_state: PlayerState {
                volume: state.volume,
                ..Default::default()
            },
            visual: Visualizer::start(),
            cfg,
            keymap: Keymap::default(),
            api,
            player,
            queue,
            covers,
            view: View::Track,
            prev_view: View::Track,
            palette: start_palette,
            cover: None,
            cover_for: None,
            queue_sel: 0,
            library: Vec::new(),
            library_sel: 0,
            library_loading: false,
            library_rows: Vec::new(),
            search_input: String::new(),
            search_editing: false,
            search_results: Vec::new(),
            suggestions: Vec::new(),
            search_sel: 0,
            searching: false,
            lyrics: None,
            lyrics_for: None,
            lyrics_scroll: 0,
            status: None,
            should_quit: false,
            cell_px: cfg_cell_px,
            cell_source,
            mpris: None,
            hits: HitRegions::default(),
            pending_seek: None,
            cover_visible: state_cover_visible,
            theme: active_theme,
            menu_open: false,
            menu_sel: 0,
            menu_screen: MenuScreen::Main,
            option_sel: 0,
            pending_play: None,
            queue_feed: None,
            graphics: cfg_graphics,
            art_on_screen: None,
            stale_cover: None,
            mpv_pos: 0,
            tx,
        }
    }

    /// Snapshot of what should survive a restart.
    pub fn runtime_state(&self) -> crate::config::State {
        crate::config::State {
            volume: self.player_state.volume,
            // Remember an eye-tuned cell size, but never a detected one.
            cover_cell: if self.cell_source == crate::art::terminal::CellSource::Config {
                [self.cell_px.0, self.cell_px.1]
            } else {
                [0, 0]
            },
            cover_visible: self.cover_visible,
            theme: self.theme.clone(),
            shuffle: self.queue.shuffle,
            repeat: match self.queue.repeat {
                RepeatMode::All => "all".into(),
                RepeatMode::One => "one".into(),
                RepeatMode::Off => "off".into(),
            },
        }
    }

    /// Switch panes. Leaving the track view means ratatui repaints the cells
    /// the sixel occupied, so the cached image is no longer valid.
    pub(crate) fn set_view(&mut self, view: View) {
        if self.view != view {
            self.clear_cover_art();
            // Only remember panes you navigate *from*, so escaping out of
            // help cannot land you back in help.
            if !matches!(self.view, View::Help | View::Lyrics) {
                self.prev_view = self.view;
            }
        }
        self.view = view;
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// Status messages expire so the footer returns to showing hints.
    pub fn status_text(&self) -> Option<&str> {
        self.status.as_ref().and_then(|(m, t)| {
            if t.elapsed() < Duration::from_secs(4) {
                Some(m.as_str())
            } else {
                None
            }
        })
    }

    // --- playback ---------------------------------------------------------

    /// Replace the queue and start playing at `start`.
    pub fn play_all(&mut self, tracks: Vec<Track>, start: usize) {
        if tracks.is_empty() {
            self.notify("nothing to play");
            return;
        }
        // Whatever was feeding the old queue no longer applies.
        self.queue_feed = None;
        self.queue.replace(tracks, start);
        self.start_current();
    }

    pub(crate) fn start_current(&mut self) {
        let Some(track) = self.queue.current().cloned() else {
            return;
        };
        if let Err(e) = self.player.play_now(&track.url()) {
            self.notify(format!("mpv: {e}"));
            return;
        }
        self.mpv_pos = 0;
        self.on_track_changed(&track);
        self.prefetch_next();
    }

    /// Queue the following track in mpv so its stream is resolved and buffered
    /// before it is needed. Without this every transition would stall for the
    /// second or two yt-dlp takes to hand over a URL.
    pub(crate) fn prefetch_next(&mut self) {
        if let Some(next) = self.queue.peek_next().cloned() {
            let _ = self.player.append(&next.url());
        }
    }

    pub(crate) fn on_track_changed(&mut self, track: &Track) {
        self.queue_sel = self.queue.current_index().unwrap_or(self.queue_sel);
        self.lyrics = None;
        self.lyrics_for = None;
        self.lyrics_scroll = 0;
        self.cover = None;
        self.cover_for = Some(track.video_id.clone());
        self.clear_cover_art();
        // kew sets the terminal title to the track by default.
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::SetTitle(format!("{} | ytkew", track.display()))
        );

        if self.cfg.cover_enabled {
            if let Some(url) = track.thumbnail.clone() {
                let tx = self.tx.clone();
                let covers = self.covers.clone();
                let vid = track.video_id.clone();
                tokio::spawn(async move {
                    match covers.load(&url).await {
                        // Render at a generous size; the UI re-renders to fit.
                        Ok(img) => {
                            let cover = crate::art::render_fit(&img, 80, 20);
                            let _ = tx.send(AppMsg::Cover {
                                video_id: vid,
                                cover: Box::new(cover),
                            });
                        }
                        Err(_) => { /* keep the placeholder */ }
                    }
                });
            }
        }
    }

    pub async fn next_track(&mut self) {
        if self.queue.peek_next().is_some() {
            // Use mpv's prefetched entry -- this is what makes skip instant.
            let _ = self.player.playlist_next().await;
        } else {
            self.notify("end of queue");
        }
    }

    pub fn prev_track(&mut self) {
        // Restart the track if we're more than a few seconds in, like every
        // other player; otherwise go back one.
        if self.player_state.time_pos > 3.0 {
            let _ = self.player.seek_absolute(0.0);
            return;
        }
        if self.queue.previous().is_some() {
            self.start_current();
        }
    }

    // --- event handling ---------------------------------------------------

    pub async fn handle_player_event(&mut self, ev: PlayerEvent) {
        match ev {
            PlayerEvent::PlaylistPos(pos) => {
                // mpv walked forward on its own (gapless advance or a forced
                // playlist-next); mirror that in our queue.
                if pos > self.mpv_pos {
                    for _ in 0..(pos - self.mpv_pos) {
                        self.queue.advance();
                    }
                    self.mpv_pos = pos;
                    if let Some(t) = self.queue.current().cloned() {
                        self.on_track_changed(&t);
                    }
                    self.prefetch_next();
                }
            }
            PlayerEvent::EndFile { reason } => {
                if reason == "error" {
                    self.notify("track failed to load, skipping");
                }
            }
            PlayerEvent::Idle => {
                if self.queue.peek_next().is_none() {
                    self.notify("queue finished");
                }
            }
            PlayerEvent::Exited => {
                self.notify("mpv exited");
                self.should_quit = true;
            }
            PlayerEvent::FileLoaded => {}
        }
    }

    pub fn handle_app_msg(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::SearchResults(tracks) => {
                self.searching = false;
                self.search_sel = 0;
                if tracks.is_empty() {
                    self.notify("no results");
                }
                self.search_results = tracks;
            }
            AppMsg::LibChildren {
                path,
                children,
                first,
                last,
            } => {
                let page: Vec<Track> = children
                    .iter()
                    .filter_map(|c| match c {
                        LibKind::Song(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                let Some(node) = self.node_at_mut(&path) else {
                    // The user collapsed or replaced the node mid-fetch.
                    return;
                };
                if first {
                    node.children.clear();
                    node.loaded = true;
                    node.expanded = true;
                }
                node.children.extend(children.into_iter().map(LibNode::new));
                // Keep the spinner until the last page, so a partly filled
                // list does not look finished.
                node.loading = !last;
                self.rebuild_library_rows();

                if self.pending_play.as_deref() == Some(path.as_slice()) {
                    // Start on the first page that has anything playable
                    // rather than waiting for a long fetch to finish.
                    let tracks = self.songs_under(&path);
                    if !tracks.is_empty() {
                        self.pending_play = None;
                        self.play_all(tracks, 0);
                        self.queue_feed = (!last).then(|| path.clone());
                    } else if last {
                        self.pending_play = None;
                        self.notify("nothing playable here");
                    }
                } else if self.queue_feed.as_deref() == Some(path.as_slice()) {
                    if !page.is_empty() {
                        self.queue.extend(page);
                        // The queue may have just gained a next track where it
                        // had none, so mpv needs to hear about it.
                        self.prefetch_next();
                    }
                    if last {
                        self.queue_feed = None;
                    }
                }
            }
            AppMsg::LibFailed { path, error } => {
                if self.pending_play.as_deref() == Some(path.as_slice()) {
                    self.pending_play = None;
                }
                if self.queue_feed.as_deref() == Some(path.as_slice()) {
                    self.queue_feed = None;
                }
                if let Some(node) = self.node_at_mut(&path) {
                    node.loading = false;
                }
                self.rebuild_library_rows();
                self.notify(error);
            }
            AppMsg::RadioTail { after, tracks } => {
                // Only extend if the seed track is still the one playing, so a
                // late radio response cannot hijack a new selection.
                if self.queue.current().map(|t| t.video_id.as_str()) == Some(after.as_str()) {
                    let existing: std::collections::HashSet<String> = self
                        .queue
                        .tracks()
                        .iter()
                        .map(|t| t.video_id.clone())
                        .collect();
                    let fresh: Vec<Track> = tracks
                        .into_iter()
                        .filter(|t| !existing.contains(&t.video_id))
                        .collect();
                    if !fresh.is_empty() {
                        self.queue.extend(fresh);
                        self.prefetch_next();
                    }
                }
            }
            AppMsg::Suggestions { query, items } => {
                if query == self.search_input.trim() {
                    self.suggestions = items;
                }
            }
            AppMsg::Cover { video_id, cover } => {
                if self.cover_for.as_deref() == Some(video_id.as_str()) {
                    if self.theme_follows_cover() {
                        self.palette = cover.palette;
                    }
                    self.cover = Some(*cover);
                }
            }
            AppMsg::Lyrics { video_id, text } => {
                if self.queue.current().map(|t| t.video_id.as_str()) == Some(video_id.as_str()) {
                    self.lyrics_for = Some(video_id);
                    self.lyrics = Some(text);
                }
            }
            AppMsg::Error(e) => {
                self.searching = false;
                self.library_loading = false;
                self.notify(e);
            }
        }
    }

    /// mpv's prefetched "next" is stale after a reorder or mode change. Reset
    /// the playlist tail without disturbing what is currently playing.
    pub(crate) fn resync_prefetch(&mut self) {
        // playlist-clear removes everything except the current entry.
        let _ = self.player.clear_playlist();
        self.mpv_pos = 0;
        self.prefetch_next();
    }

    fn selection_mut(&mut self) -> Option<&mut usize> {
        match self.view {
            View::Queue => Some(&mut self.queue_sel),
            View::Library => Some(&mut self.library_sel),
            View::Search => Some(&mut self.search_sel),
            _ => None,
        }
    }

    fn node_at(&self, path: &[usize]) -> Option<&LibNode> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.library.get(first)?;
        for &i in rest {
            node = node.children.get(i)?;
        }
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{AlbumRef, ArtistRef};

    pub(crate) fn song(title: &str) -> LibKind {
        LibKind::Song(Track {
            video_id: title.to_lowercase(),
            title: title.into(),
            artist: "A".into(),
            duration_text: "3:00".into(),
            ..Default::default()
        })
    }

    /// Build a tree without the App, to test flattening in isolation.
    pub(crate) fn flatten(nodes: &[LibNode]) -> Vec<LibRow> {
        fn walk(nodes: &[LibNode], path: &mut Vec<usize>, depth: usize, out: &mut Vec<LibRow>) {
            for (i, node) in nodes.iter().enumerate() {
                path.push(i);
                let marker = if node.kind.is_song() {
                    " "
                } else if node.loading {
                    "\u{22ef}"
                } else if node.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                out.push(LibRow {
                    path: path.clone(),
                    depth,
                    label: node.kind.label(),
                    sublabel: node.kind.sublabel(),
                    marker,
                    is_song: node.kind.is_song(),
                });
                if node.expanded {
                    walk(&node.children, path, depth + 1, out);
                }
                path.pop();
            }
        }
        let mut out = Vec::new();
        walk(nodes, &mut Vec::new(), 0, &mut out);
        out
    }

    #[test]
    pub(crate) fn collapsed_nodes_hide_their_children() {
        let mut artists = LibNode::new(LibKind::ArtistsFolder);
        artists.children = vec![LibNode::new(LibKind::Artist(ArtistRef {
            channel_id: "c1".into(),
            name: "Radiohead".into(),
            subtitle: String::new(),
        }))];
        artists.loaded = true;
        let rows = flatten(&[artists]);
        assert_eq!(rows.len(), 1, "collapsed folder shows only itself");
        assert_eq!(rows[0].marker, "\u{25b8}");
    }

    #[test]
    pub(crate) fn expanding_reveals_children_with_increasing_depth() {
        let mut album = LibNode::new(LibKind::Album(AlbumRef {
            id: "al".into(),
            title: "OK Computer".into(),
            year: "1997".into(),
        }));
        album.children = vec![LibNode::new(song("Airbag")), LibNode::new(song("Lucky"))];
        album.loaded = true;
        album.expanded = true;

        let mut artist = LibNode::new(LibKind::Artist(ArtistRef {
            channel_id: "c1".into(),
            name: "Radiohead".into(),
            subtitle: String::new(),
        }));
        artist.children = vec![album];
        artist.loaded = true;
        artist.expanded = true;

        let mut folder = LibNode::new(LibKind::ArtistsFolder);
        folder.children = vec![artist];
        folder.loaded = true;
        folder.expanded = true;

        let rows = flatten(&[folder]);
        // Artists > Radiohead > OK Computer > 2 songs
        assert_eq!(
            rows.len(),
            5,
            "{:?}",
            rows.iter().map(|r| &r.label).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].depth, 2);
        assert_eq!(rows[3].depth, 3);
        assert!(rows[3].is_song);
        assert_eq!(rows[3].path, vec![0, 0, 0, 0]);
        assert_eq!(rows[4].path, vec![0, 0, 0, 1]);
    }

    #[test]
    pub(crate) fn paths_address_the_right_node_at_every_depth() {
        let mut album = LibNode::new(LibKind::Album(AlbumRef {
            id: "al".into(),
            title: "Album".into(),
            year: String::new(),
        }));
        album.children = vec![LibNode::new(song("One")), LibNode::new(song("Two"))];
        album.expanded = true;
        album.loaded = true;
        let mut folder = LibNode::new(LibKind::AlbumsFolder);
        folder.children = vec![album];
        folder.expanded = true;
        folder.loaded = true;
        let tree = vec![folder];

        // Walk a path by hand, mirroring node_at.
        let node = &tree[0].children[0].children[1];
        assert_eq!(node.kind.label(), "A — Two");

        let rows = flatten(&tree);
        let last = rows.last().unwrap();
        assert_eq!(last.path, vec![0, 0, 1]);
    }

    #[test]
    pub(crate) fn songs_are_leaves_and_never_show_a_disclosure_marker() {
        let rows = flatten(&[LibNode::new(song("Solo"))]);
        assert_eq!(rows[0].marker, " ");
        assert!(rows[0].is_song);
        // A song counts as already loaded, so it is never fetched.
        assert!(LibNode::new(song("Solo")).loaded);
    }

    #[test]
    pub(crate) fn loading_nodes_show_a_progress_marker() {
        let mut n = LibNode::new(LibKind::ArtistsFolder);
        n.loading = true;
        let rows = flatten(&[n]);
        assert_eq!(rows[0].marker, "\u{22ef}");
    }

    #[test]
    pub(crate) fn labels_and_sublabels_describe_each_kind() {
        assert_eq!(LibKind::LikedMusic.label(), "Liked Music");
        assert_eq!(LibKind::ArtistsFolder.label(), "Artists");
        let al = LibKind::Album(AlbumRef {
            id: "x".into(),
            title: "Kid A".into(),
            year: "2000".into(),
        });
        assert_eq!(al.label(), "Kid A");
        assert_eq!(al.sublabel(), "2000");
        assert_eq!(song("Idioteque").label(), "A — Idioteque");
        assert_eq!(song("Idioteque").sublabel(), "3:00");
    }
}
