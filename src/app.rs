//! Application state, input handling and the async event loop.

use crate::api::{AlbumRef, Api, ArtistRef, Playlist};
use crate::config::{Action, Config, Keymap, VisualizerMode};
use crate::cover::{Cover, CoverLoader};
use crate::model::Track;
use crate::mpris::{metadata_for, Mpris, MprisCommand, MprisState};
use crate::palette::Palette;
use crate::player::{Player, PlayerEvent, PlayerState};
use crate::queue::{Queue, RepeatMode};
use crate::ui::View;
use crate::visual::Visualizer;
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Results of background work, delivered back to the single-threaded UI.
pub enum AppMsg {
    SearchResults(Vec<Track>),
    /// Children for the node at `path`. Addressed by path so a slow response
    /// cannot land on the wrong node after the user navigates away.
    LibChildren { path: Vec<usize>, children: Vec<LibKind> },
    LibFailed { path: Vec<usize>, error: String },
    /// A radio mix to append behind a single played track.
    RadioTail { after: String, tracks: Vec<Track> },
    /// Autocomplete for the search box, tagged with the query it answers so
    /// a slow response cannot overwrite newer typing.
    Suggestions { query: String, items: Vec<String> },
    Cover { video_id: String, cover: Box<Cover> },
    Lyrics { video_id: String, text: String },
    Error(String),
}

/// What a library tree node represents.
#[derive(Clone, Debug)]
pub enum LibKind {
    /// The `LM` auto-playlist: songs liked *in YouTube Music*.
    LikedMusic,
    /// Songs explicitly added to your library -- a different set entirely,
    /// and commonly empty even when Liked Music is not.
    LibrarySongs,
    ArtistsFolder,
    AlbumsFolder,
    PlaylistsFolder,
    Playlist(Playlist),
    Artist(ArtistRef),
    Album(AlbumRef),
    Song(Track),
}

impl LibKind {
    fn is_song(&self) -> bool {
        matches!(self, LibKind::Song(_))
    }

    fn label(&self) -> String {
        match self {
            LibKind::LikedMusic => "Liked Music".into(),
            LibKind::LibrarySongs => "Library Songs".into(),
            LibKind::ArtistsFolder => "Artists".into(),
            LibKind::AlbumsFolder => "Albums".into(),
            LibKind::PlaylistsFolder => "Playlists".into(),
            LibKind::Playlist(p) => p.title.clone(),
            LibKind::Artist(a) => a.name.clone(),
            LibKind::Album(a) => a.title.clone(),
            LibKind::Song(t) => {
                if t.artist.is_empty() {
                    t.title.clone()
                } else {
                    format!("{} — {}", t.artist, t.title)
                }
            }
        }
    }

    fn sublabel(&self) -> String {
        match self {
            LibKind::Playlist(p) => {
                if p.author.is_empty() {
                    p.track_count.clone()
                } else {
                    format!("{} · {}", p.track_count, p.author)
                }
            }
            LibKind::Artist(a) => a.subtitle.clone(),
            LibKind::Album(a) => a.year.clone(),
            LibKind::Song(t) => t.duration_text.clone(),
            _ => String::new(),
        }
    }
}

/// Regions the last frame drew, so a click can be resolved to the thing under
/// it. Recorded during render because only the renderer knows the geometry.
#[derive(Default)]
pub struct HitRegions {
    /// Tab label areas and the view each selects.
    pub tabs: Vec<(ratatui::layout::Rect, View)>,
    /// The scrollable list area, and the item index its first row shows.
    pub list: Option<(ratatui::layout::Rect, usize)>,
    /// Seekable progress bars: the whole bar including its time labels.
    pub progress: Option<ratatui::layout::Rect>,
    /// Where the bar's track actually starts and how wide it is, since the
    /// labels either side are not seekable.
    pub progress_track: Option<(u16, u16)>,
}

/// Entries in the overlay menu, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Options,
    Help,
    Quit,
}

/// What a keypress did while the menu was open.
enum MenuOutcome {
    /// Handled entirely by the menu.
    Consumed,
    /// Not a menu key; the menu closed and normal handling should proceed.
    Fallthrough,
}

pub const MENU_ITEMS: [MenuItem; 3] = [MenuItem::Options, MenuItem::Help, MenuItem::Quit];

/// Which pane of the overlay is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScreen {
    Main,
    Options,
}

/// A row in the options pane. Each is a fixed list of choices stepped through
/// with the left and right arrows, which is how btop's options read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Theme,
    Renderer,
    ShowCover,
    Visualizer,
    Shuffle,
    Repeat,
    AutoplayRadio,
}

pub const SETTINGS: [Setting; 7] = [
    Setting::Theme,
    Setting::Renderer,
    Setting::ShowCover,
    Setting::Visualizer,
    Setting::Shuffle,
    Setting::Repeat,
    Setting::AutoplayRadio,
];

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Setting::Theme => "Theme",
            Setting::Renderer => "Cover renderer",
            Setting::ShowCover => "Show cover art",
            Setting::Visualizer => "Visualizer",
            Setting::Shuffle => "Shuffle",
            Setting::Repeat => "Repeat",
            Setting::AutoplayRadio => "Autoplay radio",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Setting::Theme => {
                "Colours for borders, text and accents. \"cover\" samples them from the album art."
            }
            Setting::Renderer => {
                "How album art is drawn. Kitty sizes in cells and needs no tuning; sixel needs an accurate cell size; blocks work anywhere."
            }
            Setting::ShowCover => "Show the artwork at all. Same as pressing b.",
            Setting::Visualizer => {
                "Spectrum style. Braille packs four levels per cell; off reclaims the rows."
            }
            Setting::Shuffle => "Play the queue in a random order.",
            Setting::Repeat => "Off stops at the end, all wraps, one loops the track.",
            Setting::AutoplayRadio => {
                "After playing a single search hit, append YouTube's radio mix so it keeps going."
            }
        }
    }
}

/// The palette a theme name resolves to. `cover` has no fixed colours -- it
/// starts from the accent and is replaced once artwork loads.
fn theme_palette(name: &str, cfg: &Config, accent: u8) -> Palette {
    if name.eq_ignore_ascii_case("custom") {
        if let Some(p) = crate::theme::from_hex(&cfg.theme_colors) {
            return p;
        }
    }
    match crate::theme::find(name) {
        Some(t) => t.palette(),
        None => Palette::from_ansi(accent),
    }
}

/// How the cover is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graphics {
    /// Kitty graphics: sized in cells, so no cell pixel size is involved.
    /// Preferred wherever it is available.
    Kitty,
    /// Sixel: needs an accurate cell pixel size.
    Sixel,
    /// Unicode half-blocks inside ratatui's own buffer.
    None,
}

impl Graphics {
    fn resolve(cfg: &Config, cell_source: crate::sixel::CellSource) -> Self {
        // Kitty first: it is the only protocol that sidesteps cell-size
        // detection entirely, which is what every sizing bug here came from.
        if cfg
            .cover_mode
            .uses_kitty(crate::kitty::terminal_supports_kitty())
        {
            return Graphics::Kitty;
        }
        if cfg
            .cover_mode
            .uses_sixel(crate::sixel::sixel_recommended(cell_source))
        {
            return Graphics::Sixel;
        }
        Graphics::None
    }
}

/// A node in the lazily-loaded library tree.
pub struct LibNode {
    pub kind: LibKind,
    pub expanded: bool,
    pub loading: bool,
    /// Whether children have been fetched. Distinguishes "not yet loaded"
    /// from "loaded and genuinely empty".
    pub loaded: bool,
    pub children: Vec<LibNode>,
}

impl LibNode {
    fn new(kind: LibKind) -> Self {
        let loaded = kind.is_song();
        Self {
            kind,
            expanded: false,
            loading: false,
            loaded,
            children: Vec::new(),
        }
    }
}

/// One rendered line of the tree, precomputed so the view does no traversal
/// and no allocation per frame.
pub struct LibRow {
    pub path: Vec<usize>,
    pub depth: usize,
    pub label: String,
    pub sublabel: String,
    /// Disclosure indicator: expanded, collapsed, loading, or a leaf.
    pub marker: &'static str,
    pub is_song: bool,
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
    pub cell_source: crate::sixel::CellSource,

    /// D-Bus presence. None when there is no session bus, e.g. over plain ssh
    /// -- media keys are a bonus, never a requirement.
    pub mpris: Option<Mpris>,

    /// Clickable regions from the last frame.
    pub hits: HitRegions,

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
    /// Which graphics protocol to draw the cover with, if any.
    graphics: Graphics,
    /// A region just blanked, which the next frame must treat as empty so
    /// ratatui's model matches the screen.
    stale_cover: Option<ratatui::layout::Rect>,
    /// What image is currently on screen: (video id, rect, protocol). The
    /// protocol is recorded because removing a kitty placement needs an
    /// explicit delete, and by the time we remove it the mode may already
    /// have been cycled to something else.
    sixel_on_screen: Option<(String, ratatui::layout::Rect, Graphics)>,


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
        let (cfg_cell_px, cell_source) = crate::sixel::detect_cell_size(match cfg.cell_px {
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
            cover_visible: state_cover_visible,
            theme: active_theme,
            menu_open: false,
            menu_sel: 0,
            menu_screen: MenuScreen::Main,
            option_sel: 0,
            pending_play: None,
            graphics: cfg_graphics,
            sixel_on_screen: None,
            stale_cover: None,
            mpv_pos: 0,
            tx,
        }
    }

    /// Recompute the palette for the current theme. Under `cover` this waits
    /// for artwork; the accent stands in until then.
    fn apply_theme(&mut self) {
        if self.theme.eq_ignore_ascii_case(crate::theme::COVER) {
            // Fall back to the cover we already have, if any.
            self.palette = match &self.cover {
                Some(c) => c.palette,
                None => Palette::from_ansi(self.cfg.accent_color),
            };
        } else {
            self.palette = theme_palette(&self.theme, &self.cfg, self.cfg.accent_color);
        }
        self.invalidate_sixel();
    }

    /// True when colours should follow the artwork.
    fn theme_follows_cover(&self) -> bool {
        self.theme.eq_ignore_ascii_case(crate::theme::COVER)
    }

    /// Adjust the assumed cell height by `delta` px, deriving the width from
    /// the current aspect so the art stays square.
    ///
    /// This exists because no automatic measurement is reliable here: zellij
    /// has an open regression (zellij-org/zellij#3372) that renders sixel at
    /// double height, and nothing the terminal reports reflects that. Letting
    /// the user resize while watching is the only approach that converges.
    fn nudge_cover(&mut self, delta: i32) {
        let (w, h) = self.cell_px;
        let ratio = w as f32 / h.max(1) as f32;
        let new_h = (h as i32 + delta).clamp(6, 80) as u16;
        let new_w = ((new_h as f32 * ratio).round() as u16).max(3);
        self.cell_px = (new_w, new_h);
        // A hand-tuned value is pinned, and so is trusted and persisted.
        self.cell_source = crate::sixel::CellSource::Config;
        self.cfg.cell_px = [new_w, new_h];
        self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
        self.invalidate_sixel();
        self.notify(format!(
            "cover cell {new_w}x{new_h} — [ smaller, ] bigger; saved on exit"
        ));
    }

    /// Measure the terminal's real graphics cell size.
    ///
    /// Must be called once the alternate screen is active but before any
    /// input reader exists, since it writes a probe and reads the reply.
    /// Skipped when the user pinned `cell_px` themselves.
    pub fn calibrate_cells(&mut self) {
        if self.cell_source == crate::sixel::CellSource::Config {
            return;
        }
        // Kitty needs no cell size, so do not probe when it is in play.
        if self.graphics == Graphics::Kitty || !self.cfg.cover_mode.uses_sixel(true) {
            return;
        }
        match crate::sixel::calibrate(self.cell_px) {
            Some((w, h)) => {
                self.cell_px = (w, h);
                self.cell_source = crate::sixel::CellSource::Calibrated;
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
            None => {
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
        }
        if self.graphics == Graphics::None
            && self.cfg.cover_mode == crate::config::CoverMode::Auto
            && crate::sixel::terminal_supports_sixel()
        {
            if let Some(mux) = crate::sixel::multiplexer() {
                self.notify(format!(
                    "cover: half-blocks — sixel renders at the wrong size under {mux} \
                     (zellij#3372). Press b for sixel, then [ / ] to resize."
                ));
            }
        }
        self.invalidate_sixel();
    }

    /// True when the cover is drawn by a graphics protocol rather than into
    /// ratatui's buffer. Requires a loaded image -- there is no placeholder.
    pub fn graphics_active(&self) -> bool {
        self.cover_visible && self.graphics != Graphics::None && self.cover.is_some()
    }


    /// Take the region that was blanked, if any, so the renderer can reset
    /// ratatui's view of it.
    pub fn take_stale_cover(&mut self) -> Option<ratatui::layout::Rect> {
        self.stale_cover.take()
    }

    /// Emit the cover with the active graphics protocol, if what is on screen
    /// is stale.
    ///
    /// Called after the ratatui frame, because ratatui has no concept of
    /// pixel graphics: the cells are reserved during draw and the image is
    /// written over them here.
    pub fn paint_graphics(&mut self, rect: ratatui::layout::Rect) -> std::io::Result<()> {
        use std::io::Write;

        let Some(cover) = &self.cover else {
            return Ok(());
        };
        let Some(video_id) = &self.cover_for else {
            return Ok(());
        };
        // Nothing changed -- repainting would just burn bandwidth and flicker.
        if self
            .sixel_on_screen
            .as_ref()
            .is_some_and(|(id, r, kind)| id == video_id && *r == rect && *kind == self.graphics)
        {
            return Ok(());
        }

        let payload = match self.graphics {
            Graphics::Kitty => {
                // The terminal scales to the cell box, so send a fixed decent
                // resolution rather than trying to compute pixel dimensions.
                const MAX_PX: u32 = 512;
                let src = cover.source.as_ref();
                let scaled = if src.width() > MAX_PX || src.height() > MAX_PX {
                    image::imageops::resize(
                        src,
                        MAX_PX,
                        MAX_PX,
                        image::imageops::FilterType::Lanczos3,
                    )
                } else {
                    src.clone()
                };
                match crate::kitty::draw(&scaled, rect.width, rect.height) {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
            Graphics::Sixel => {
                let (cw, ch) = self.cell_px;
                let px_w = rect.width as u32 * cw as u32;
                let px_h = rect.height as u32 * ch as u32;
                if px_w == 0 || px_h == 0 {
                    return Ok(());
                }
                let resized = image::imageops::resize(
                    cover.source.as_ref(),
                    px_w,
                    px_h,
                    image::imageops::FilterType::Lanczos3,
                );
                crate::sixel::encode(&resized)
            }
            Graphics::None => return Ok(()),
        };

        let mut out = std::io::stdout().lock();
        // Both protocols draw from the cursor, so park it at the top-left of
        // the reserved area. Rows and columns are 1-based in CUP.
        write!(out, "\x1b[{};{}H", rect.y + 1, rect.x + 1)?;
        out.write_all(payload.as_bytes())?;
        out.flush()?;

        self.sixel_on_screen = Some((video_id.clone(), rect, self.graphics));
        Ok(())
    }

    /// Remove whatever image is on screen.
    ///
    /// Both protocols need explicit removal, for different reasons. A kitty
    /// placement belongs to the terminal rather than the cell grid, so
    /// drawing text over it does nothing -- it needs a delete command. Sixel
    /// pixels do live in the grid, but the cells were marked as skipped and
    /// so still hold their previous contents; if the next frame happens to
    /// draw the same thing there, ratatui's diff emits nothing and the art
    /// stays put. So the region is blanked directly rather than relying on
    /// the diff.
    ///
    /// Deliberately not `Terminal::clear()`: that queries the cursor position
    /// and blocks on the reply, which races the input reader for stdin.
    pub fn invalidate_sixel(&mut self) {
        use std::io::Write;
        let Some((_, rect, kind)) = self.sixel_on_screen.take() else {
            return;
        };
        let mut out = std::io::stdout().lock();
        if kind == Graphics::Kitty {
            let _ = out.write_all(crate::kitty::delete().as_bytes());
        }
        // Blank the cells the image covered, and tell ratatui they are blank
        // so its next diff agrees with what is actually on screen.
        let blanks = " ".repeat(rect.width as usize);
        for row in 0..rect.height {
            let _ = write!(out, "\x1b[{};{}H{}", rect.y + row + 1, rect.x + 1, blanks);
        }
        let _ = out.flush();
        self.stale_cover = Some(rect);
    }

    /// Current transport state as MPRIS describes it.
    pub fn mpris_snapshot(&self) -> MprisState {
        use mpris_server::{LoopStatus, PlaybackStatus};
        let track = self.queue.current();
        let status = match track {
            None => PlaybackStatus::Stopped,
            Some(_) if self.player_state.paused => PlaybackStatus::Paused,
            Some(_) => PlaybackStatus::Playing,
        };
        MprisState {
            status,
            metadata: track
                .map(|t| metadata_for(t, self.queue.current_index().unwrap_or(0)))
                .unwrap_or_default(),
            track_key: track.map(|t| t.video_id.clone()).unwrap_or_default(),
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

    /// Push current state onto D-Bus. Cheap when nothing changed.
    pub async fn sync_mpris(&self) {
        let Some(m) = &self.mpris else { return };
        // Position is polled by clients, so update it without a signal.
        m.set_position(self.player_state.time_pos);
        m.publish(self.mpris_snapshot()).await;
    }

    /// Route a mouse event to whatever was drawn under the pointer.
    pub async fn handle_mouse(&mut self, ev: crossterm::event::MouseEvent) -> Result<()> {
        use crossterm::event::{MouseButton, MouseEventKind};
        let (col, row) = (ev.column, ev.row);

        match ev.kind {
            MouseEventKind::ScrollUp => {
                // Scrolling over a list scrolls it; anywhere else nudges volume,
                // which is what a media player's wheel usually does.
                if self.point_in_list(col, row) {
                    self.move_selection(-3);
                } else {
                    self.player.add_volume(self.cfg.volume_step).await?;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.point_in_list(col, row) {
                    self.move_selection(3);
                } else {
                    self.player.add_volume(-self.cfg.volume_step).await?;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(view) = self.tab_at(col, row) {
                    self.set_view(view);
                    if view == View::Library {
                        self.ensure_library();
                    }
                    return Ok(());
                }
                if let Some(pct) = self.progress_at(col, row) {
                    let d = self
                        .player_state
                        .duration
                        .max(self.queue.current().and_then(|t| t.duration).unwrap_or(0.0));
                    if d > 0.0 {
                        self.player.seek_absolute(d * pct)?;
                    }
                    return Ok(());
                }
                if let Some(index) = self.row_at(col, row) {
                    // First click selects, a click on the current selection
                    // activates -- the same feel as a file manager.
                    let already = self.selection() == Some(index);
                    self.set_selection(index);
                    if already {
                        self.activate_selection(false);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.handle_action(Action::PlayPause).await?;
            }
            _ => {}
        }
        Ok(())
    }

    fn tab_at(&self, col: u16, row: u16) -> Option<View> {
        self.hits
            .tabs
            .iter()
            .find(|(r, _)| {
                col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
            })
            .map(|(_, v)| *v)
    }

    fn point_in_list(&self, col: u16, row: u16) -> bool {
        self.hits.list.is_some_and(|(r, _)| {
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        })
    }

    /// Item index under the pointer, if it is over a list row.
    fn row_at(&self, col: u16, row: u16) -> Option<usize> {
        let (r, start) = self.hits.list?;
        if col < r.x || col >= r.x + r.width || row < r.y || row >= r.y + r.height {
            return None;
        }
        let index = start + (row - r.y) as usize;
        (index < self.list_len()).then_some(index)
    }

    /// Fraction along a progress bar that was clicked, if any.
    fn progress_at(&self, col: u16, row: u16) -> Option<f64> {
        let r = self.hits.progress?;
        if row < r.y || row >= r.y + r.height {
            return None;
        }
        let (track_x, track_w) = self.hits.progress_track?;
        if track_w == 0 || col < track_x || col >= track_x + track_w {
            return None;
        }
        Some((col - track_x) as f64 / track_w as f64)
    }

    fn selection(&self) -> Option<usize> {
        match self.view {
            View::Queue => Some(self.queue_sel),
            View::Library => Some(self.library_sel),
            View::Search => Some(self.search_sel),
            _ => None,
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
                if let Some(m) = &self.mpris {
                    m.seeked(secs).await;
                }
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

    /// Snapshot of what should survive a restart.
    pub fn runtime_state(&self) -> crate::config::State {
        crate::config::State {
            volume: self.player_state.volume,
            // Remember an eye-tuned cell size, but never a detected one.
            cover_cell: if self.cell_source == crate::sixel::CellSource::Config {
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
    fn set_view(&mut self, view: View) {
        if self.view != view {
            self.invalidate_sixel();
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
        self.queue.replace(tracks, start);
        self.start_current();
    }

    fn start_current(&mut self) {
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
    fn prefetch_next(&mut self) {
        if let Some(next) = self.queue.peek_next().cloned() {
            let _ = self.player.append(&next.url());
        }
    }

    fn on_track_changed(&mut self, track: &Track) {
        self.queue_sel = self.queue.current_index().unwrap_or(self.queue_sel);
        self.lyrics = None;
        self.lyrics_for = None;
        self.lyrics_scroll = 0;
        self.cover = None;
        self.cover_for = Some(track.video_id.clone());
        self.invalidate_sixel();
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
                            let cover = crate::cover::render_fit(&img, 80, 20);
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
            AppMsg::LibChildren { path, children } => {
                if let Some(node) = self.node_at_mut(&path) {
                    node.loading = false;
                    node.loaded = true;
                    node.expanded = true;
                    node.children = children.into_iter().map(LibNode::new).collect();
                }
                self.rebuild_library_rows();
                // A "play all" that was waiting on this fetch.
                if self.pending_play.as_deref() == Some(path.as_slice()) {
                    self.pending_play = None;
                    let tracks = self.songs_under(&path);
                    if tracks.is_empty() {
                        self.notify("nothing playable here");
                    } else {
                        self.play_all(tracks, 0);
                    }
                }
            }
            AppMsg::LibFailed { path, error } => {
                if self.pending_play.as_deref() == Some(path.as_slice()) {
                    self.pending_play = None;
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
                    let existing: std::collections::HashSet<String> =
                        self.queue.tracks().iter().map(|t| t.video_id.clone()).collect();
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

    /// The word rendered in block letters for a top-level menu row.
    pub fn menu_word(item: MenuItem) -> &'static str {
        match item {
            MenuItem::Options => "OPTIONS",
            MenuItem::Help => "HELP",
            MenuItem::Quit => "QUIT",
        }
    }

    /// The choices for a setting and which one is current.
    pub fn setting_choices(&self, s: Setting) -> (Vec<String>, usize) {
        let pick = |names: &[&str], cur: &str| -> (Vec<String>, usize) {
            let v: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            let i = v.iter().position(|n| n.eq_ignore_ascii_case(cur)).unwrap_or(0);
            (v, i)
        };
        match s {
            Setting::Theme => {
                let names = crate::theme::names();
                let i = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&self.theme))
                    .unwrap_or(0);
                (names.iter().map(|n| n.to_string()).collect(), i)
            }
            Setting::Renderer => pick(
                &["auto", "kitty", "sixel", "blocks"],
                self.cfg.cover_mode.name(),
            ),
            Setting::ShowCover => pick(&["on", "off"], if self.cover_visible { "on" } else { "off" }),
            Setting::Visualizer => pick(
                &["bars", "braille", "off"],
                match self.cfg.visualizer_mode {
                    crate::config::VisualizerMode::Bars => "bars",
                    crate::config::VisualizerMode::Braille => "braille",
                    crate::config::VisualizerMode::Off => "off",
                },
            ),
            Setting::Shuffle => pick(&["off", "on"], if self.queue.shuffle { "on" } else { "off" }),
            Setting::Repeat => pick(
                &["off", "all", "one"],
                match self.queue.repeat {
                    RepeatMode::Off => "off",
                    RepeatMode::All => "all",
                    RepeatMode::One => "one",
                },
            ),
            Setting::AutoplayRadio => {
                pick(&["on", "off"], if self.cfg.autoplay_radio { "on" } else { "off" })
            }
        }
    }

    /// Current value of a setting, for display.
    pub fn setting_value(&self, s: Setting) -> String {
        let (choices, i) = self.setting_choices(s);
        choices.get(i).cloned().unwrap_or_default()
    }

    /// Step a setting left or right, wrapping.
    pub fn adjust_setting(&mut self, s: Setting, delta: i32) {
        let (choices, i) = self.setting_choices(s);
        if choices.is_empty() {
            return;
        }
        let n = choices.len() as i32;
        let next = (((i as i32 + delta) % n) + n) % n;
        let value = choices[next as usize].clone();
        self.apply_setting(s, &value);
    }

    fn apply_setting(&mut self, s: Setting, value: &str) {
        use crate::config::{CoverMode, VisualizerMode};
        match s {
            Setting::Theme => {
                self.theme = value.to_string();
                self.apply_theme();
            }
            Setting::Renderer => {
                self.invalidate_sixel();
                self.cfg.cover_mode = match value {
                    "kitty" => CoverMode::Kitty,
                    "sixel" => CoverMode::Sixel,
                    "blocks" => CoverMode::Blocks,
                    _ => CoverMode::Auto,
                };
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
            Setting::ShowCover => {
                self.invalidate_sixel();
                self.cover_visible = value == "on";
            }
            Setting::Visualizer => {
                self.cfg.visualizer_mode = match value {
                    "braille" => VisualizerMode::Braille,
                    "off" => VisualizerMode::Off,
                    _ => VisualizerMode::Bars,
                };
                // The visualizer changes the layout, so any art must be redrawn.
                self.invalidate_sixel();
            }
            Setting::Shuffle => {
                if (value == "on") != self.queue.shuffle {
                    self.queue.toggle_shuffle();
                    self.resync_prefetch();
                }
            }
            Setting::Repeat => {
                self.queue.repeat = match value {
                    "all" => RepeatMode::All,
                    "one" => RepeatMode::One,
                    _ => RepeatMode::Off,
                };
                let _ = self
                    .player
                    .set_loop_file(self.queue.repeat == RepeatMode::One);
                self.resync_prefetch();
            }
            Setting::AutoplayRadio => self.cfg.autoplay_radio = value == "on",
        }
    }

    /// Keys while the overlay is open.
    fn handle_menu_action(&mut self, action: Action) -> MenuOutcome {
        if !self.menu_open {
            return MenuOutcome::Fallthrough;
        }
        match self.menu_screen {
            MenuScreen::Main => match action {
                Action::ScrollUp => self.menu_sel = self.menu_sel.saturating_sub(1),
                Action::ScrollDown => {
                    self.menu_sel = (self.menu_sel + 1).min(MENU_ITEMS.len() - 1)
                }
                Action::ToggleMenu => self.menu_open = false,
                Action::Enqueue | Action::EnqueueAndPlay => {
                    match MENU_ITEMS[self.menu_sel.min(MENU_ITEMS.len() - 1)] {
                        MenuItem::Options => {
                            self.menu_screen = MenuScreen::Options;
                            self.option_sel = 0;
                        }
                        MenuItem::Help => {
                            self.menu_open = false;
                            self.set_view(View::Help);
                        }
                        MenuItem::Quit => self.should_quit = true,
                    }
                }
                Action::Quit => self.should_quit = true,
                _ => {
                    self.menu_open = false;
                    return MenuOutcome::Fallthrough;
                }
            },
            MenuScreen::Options => match action {
                Action::ScrollUp => self.option_sel = self.option_sel.saturating_sub(1),
                Action::ScrollDown => {
                    self.option_sel = (self.option_sel + 1).min(SETTINGS.len() - 1)
                }
                // Left and right step the value, which is what the arrows
                // beside the selected row advertise.
                Action::Prev => {
                    let s = SETTINGS[self.option_sel.min(SETTINGS.len() - 1)];
                    self.adjust_setting(s, -1);
                }
                Action::Next | Action::Enqueue | Action::EnqueueAndPlay => {
                    let s = SETTINGS[self.option_sel.min(SETTINGS.len() - 1)];
                    self.adjust_setting(s, 1);
                }
                // Escape backs out to the menu rather than closing outright.
                Action::ToggleMenu => self.menu_screen = MenuScreen::Main,
                Action::Quit => self.should_quit = true,
                _ => {}
            },
        }
        MenuOutcome::Consumed
    }

    /// Play everything the current view implies: the selected container in
    /// the library, all search results, or the queue from its start.
    pub fn play_all_in_context(&mut self) {
        match self.view {
            View::Search => {
                let all = self.search_results.clone();
                if all.is_empty() {
                    self.notify("nothing to play");
                } else {
                    self.play_all(all, 0);
                }
            }
            View::Library => self.play_selected_library_node(),
            _ => {
                if self.queue.is_empty() {
                    self.notify("queue is empty");
                } else {
                    self.queue.jump_to(0);
                    self.start_current();
                }
            }
        }
    }

    /// Play a library node's whole contents, fetching them first if needed.
    fn play_selected_library_node(&mut self) {
        let Some(row) = self.library_rows.get(self.library_sel) else {
            return;
        };
        let path = row.path.clone();
        // A song plays its siblings from that point, which is what "play all"
        // means when the cursor is inside a list.
        if row.is_song {
            self.activate_selection(true);
            return;
        }
        let Some(node) = self.node_at(&path) else {
            return;
        };
        if node.loaded {
            let tracks = self.songs_under(&path);
            if tracks.is_empty() {
                self.notify("nothing playable here");
            } else {
                self.play_all(tracks, 0);
            }
            return;
        }
        // Not fetched yet: request it and play once it lands.
        if let Some(n) = self.node_at_mut(&path) {
            if n.loading {
                return;
            }
            n.loading = true;
        }
        let kind = self.node_at(&path).map(|n| n.kind.clone());
        if let Some(kind) = kind {
            self.pending_play = Some(path.clone());
            self.notify("loading…");
            self.rebuild_library_rows();
            self.spawn_library_load(path, kind);
        }
    }

    /// Every song directly under a node, in display order.
    fn songs_under(&self, path: &[usize]) -> Vec<Track> {
        let Some(node) = self.node_at(path) else {
            return Vec::new();
        };
        node.children
            .iter()
            .filter_map(|c| match &c.kind {
                LibKind::Song(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    pub async fn handle_action(&mut self, action: Action) -> Result<()> {
        match self.handle_menu_action(action) {
            MenuOutcome::Consumed => return Ok(()),
            MenuOutcome::Fallthrough => {}
        }
        match action {
            Action::Quit => self.should_quit = true,
            Action::PlayPause => {
                if self.queue.current().is_none() {
                    self.notify("nothing queued");
                } else {
                    self.player.toggle_pause().await?;
                }
            }
            Action::Stop => {
                let _ = self.player.stop();
            }
            Action::Next => self.next_track().await,
            Action::Prev => self.prev_track(),
            Action::SeekForward => self.player.seek(self.cfg.seek_step)?,
            Action::SeekBack => self.player.seek(-self.cfg.seek_step)?,
            Action::VolumeUp => {
                self.player.add_volume(self.cfg.volume_step).await?;
            }
            Action::VolumeDown => {
                self.player.add_volume(-self.cfg.volume_step).await?;
            }
            Action::Shuffle => {
                let on = self.queue.toggle_shuffle();
                // The upcoming track changed, so re-prime mpv's prefetch.
                self.resync_prefetch();
                self.notify(if on { "shuffle on" } else { "shuffle off" });
            }
            Action::ToggleRepeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                // Repeat-one is cheapest handled inside mpv.
                let _ = self
                    .player
                    .set_loop_file(self.queue.repeat == RepeatMode::One);
                self.resync_prefetch();
                self.notify(match self.queue.repeat {
                    RepeatMode::Off => "repeat off",
                    RepeatMode::All => "repeat all",
                    RepeatMode::One => "repeat one",
                });
            }
            Action::CycleVisualizer => {
                self.cfg.visualizer_mode = self.cfg.visualizer_mode.cycle();
                if !self.visual.available && self.cfg.visualizer_mode != VisualizerMode::Off {
                    self.notify("no audio capture available for the visualizer");
                }
            }
            Action::ToggleAscii => {
                // Just show or hide. Which renderer to use is a setting, not
                // something to cycle past on the way to turning art off.
                self.invalidate_sixel();
                self.cover_visible = !self.cover_visible;
                self.notify(if self.cover_visible {
                    format!("cover on ({})", self.cfg.cover_mode.name())
                } else {
                    "cover off".into()
                });
            }
            Action::NextView => self.set_view(self.view.next()),
            Action::PrevView => self.set_view(self.view.prev()),
            Action::ShowQueue => self.set_view(View::Queue),
            Action::ShowTrack => self.set_view(View::Track),
            Action::ShowHelp => self.set_view(View::Help),
            Action::ShowLibrary => {
                self.set_view(View::Library);
                self.ensure_library();
            }
            Action::ShowSearch => {
                self.set_view(View::Search);
                self.search_editing = true;
            }
            Action::ShowLyrics => {
                self.set_view(View::Lyrics);
                self.ensure_lyrics();
            }
            Action::ScrollUp => self.move_selection(-1),
            Action::ScrollDown => self.move_selection(1),
            Action::PageUp => self.move_selection(-10),
            Action::PageDown => self.move_selection(10),
            Action::Top => self.set_selection(0),
            Action::Bottom => self.set_selection(usize::MAX),
            Action::Enqueue => self.activate_selection(false),
            Action::EnqueueAndPlay => self.activate_selection(true),
            Action::Remove => self.remove_selection(),
            Action::ClearQueue => {
                self.queue.clear();
                let _ = self.player.stop();
                let _ = self.player.clear_playlist();
                self.cover = None;
                self.notify("queue cleared");
            }
            Action::MoveUp => {
                if self.view == View::Queue {
                    if let Some(n) = self.queue.move_up(self.queue_sel) {
                        self.queue_sel = n;
                        self.resync_prefetch();
                    }
                }
            }
            Action::MoveDown => {
                if self.view == View::Queue {
                    if let Some(n) = self.queue.move_down(self.queue_sel) {
                        self.queue_sel = n;
                        self.resync_prefetch();
                    }
                }
            }
            Action::ToggleLike => self.like_current(),
            Action::StartRadio => self.radio_from_current(),
            Action::PlayAll => self.play_all_in_context(),
            Action::ToggleMenu => {
                // Help and lyrics are overlays in their own right; escape
                // dismisses them rather than opening the menu over the top.
                if matches!(self.view, View::Help | View::Lyrics) {
                    let back = self.prev_view;
                    self.set_view(back);
                } else {
                    self.menu_open = !self.menu_open;
                    self.menu_sel = 0;
                    self.menu_screen = MenuScreen::Main;
                }
            }
            Action::CoverSmaller => self.nudge_cover(-2),
            Action::CoverBigger => self.nudge_cover(2),
        }
        Ok(())
    }

    /// mpv's prefetched "next" is stale after a reorder or mode change. Reset
    /// the playlist tail without disturbing what is currently playing.
    fn resync_prefetch(&mut self) {
        // playlist-clear removes everything except the current entry.
        let _ = self.player.clear_playlist();
        self.mpv_pos = 0;
        self.prefetch_next();
    }

    // --- selection --------------------------------------------------------

    pub(crate) fn list_len(&self) -> usize {
        match self.view {
            View::Queue => self.queue.len(),
            View::Library => self.library_rows.len(),
            View::Search => self.search_results.len(),
            _ => 0,
        }
    }

    fn selection_mut(&mut self) -> Option<&mut usize> {
        match self.view {
            View::Queue => Some(&mut self.queue_sel),
            View::Library => Some(&mut self.library_sel),
            View::Search => Some(&mut self.search_sel),
            _ => None,
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.view == View::Lyrics {
            self.lyrics_scroll = self.lyrics_scroll.saturating_add_signed(delta as i16);
            return;
        }
        let len = self.list_len();
        if len == 0 {
            return;
        }
        if let Some(sel) = self.selection_mut() {
            let next = (*sel as isize + delta).clamp(0, len as isize - 1);
            *sel = next as usize;
        }
    }

    pub(crate) fn set_selection(&mut self, to: usize) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        if let Some(sel) = self.selection_mut() {
            *sel = to.min(len - 1);
        }
    }

    /// Enter appends to the queue without interrupting playback; alt+enter
    /// (`jump`) appends and switches to it at once. That is kew's split
    /// between MSG_ENQUEUE and MSG_ENQUEUEANDPLAY.
    pub(crate) fn activate_selection(&mut self, jump: bool) {
        match self.view {
            View::Queue => {
                if self.queue_sel < self.queue.len() {
                    let i = self.queue_sel;
                    if self.queue.jump_to(i).is_some() {
                        self.start_current();
                    }
                }
            }
            View::Search => {
                if let Some(track) = self.search_results.get(self.search_sel).cloned() {
                    self.enqueue_track(track, jump);
                }
            }
            View::Library => self.activate_library_row(jump),
            _ => {}
        }
    }

    /// Append one track. Starts playback if nothing is going, so a first
    /// Enter on a fresh queue still does the obvious thing.
    fn enqueue_track(&mut self, track: Track, jump: bool) {
        let was_idle = self.queue.current().is_none();
        let title = track.title.clone();
        let seed = track.video_id.clone();
        self.queue.push(track);
        let last = self.queue.len() - 1;

        if was_idle || jump {
            if self.queue.jump_to(last).is_some() {
                self.start_current();
            }
            if self.cfg.autoplay_radio && was_idle {
                self.append_radio(&seed);
            }
        } else {
            // Only the upcoming entry changed; re-prime mpv's prefetch.
            self.resync_prefetch();
            self.notify(format!("queued {title}"));
        }
    }

    fn remove_selection(&mut self) {
        if self.view == View::Queue && self.queue_sel < self.queue.len() {
            self.queue.remove(self.queue_sel);
            if self.queue_sel >= self.queue.len() {
                self.queue_sel = self.queue.len().saturating_sub(1);
            }
            self.resync_prefetch();
        }
    }

    // --- library ----------------------------------------------------------

    /// Build the root of the tree. Children load on demand.
    pub fn ensure_library(&mut self) {
        if !self.library.is_empty() {
            return;
        }
        if !self.api.is_authenticated() {
            self.notify("library needs auth -- run `ytkew --auth cookie`");
            return;
        }
        self.library = vec![
            LibNode::new(LibKind::LikedMusic),
            LibNode::new(LibKind::LibrarySongs),
            LibNode::new(LibKind::ArtistsFolder),
            LibNode::new(LibKind::AlbumsFolder),
            LibNode::new(LibKind::PlaylistsFolder),
        ];
        self.rebuild_library_rows();
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut LibNode> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.library.get_mut(first)?;
        for &i in rest {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }

    fn node_at(&self, path: &[usize]) -> Option<&LibNode> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.library.get(first)?;
        for &i in rest {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// Flatten the visible tree into display rows. Called only when the tree
    /// changes, so rendering stays allocation-free.
    pub fn rebuild_library_rows(&mut self) {
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
        let mut rows = Vec::new();
        walk(&self.library, &mut Vec::new(), 0, &mut rows);
        self.library_rows = rows;
        if self.library_sel >= self.library_rows.len() {
            self.library_sel = self.library_rows.len().saturating_sub(1);
        }
    }

    /// Enter/expand the selected row, or play it if it is a song.
    fn activate_library_row(&mut self, jump: bool) {
        let Some(row) = self.library_rows.get(self.library_sel) else {
            return;
        };
        let path = row.path.clone();

        // Songs play in the context of their siblings, so selecting track 4
        // of an album queues the whole album from there.
        if row.is_song {
            let Some((parent_path, &index)) = path.split_last().map(|(i, p)| (p, i)) else {
                return;
            };
            let siblings: Vec<Track> = match self.node_at(parent_path) {
                Some(parent) => parent
                    .children
                    .iter()
                    .filter_map(|c| match &c.kind {
                        LibKind::Song(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect(),
                None => return,
            };
            if siblings.is_empty() {
                return;
            }
            if jump || self.queue.current().is_none() {
                let start = index.min(siblings.len() - 1);
                self.play_all(siblings, start);
            } else if let Some(t) = siblings.get(index).cloned() {
                self.enqueue_track(t, false);
            }
            return;
        }

        // alt+enter on a container plays its whole contents rather than
        // just opening it.
        if jump {
            self.play_selected_library_node();
            return;
        }
        // Containers: collapse if open, expand if already fetched, else load.
        let Some(node) = self.node_at_mut(&path) else {
            return;
        };
        if node.expanded {
            node.expanded = false;
            self.rebuild_library_rows();
            return;
        }
        if node.loaded {
            node.expanded = true;
            self.rebuild_library_rows();
            return;
        }
        if node.loading {
            return;
        }
        node.loading = true;
        let kind = node.kind.clone();
        self.rebuild_library_rows();
        self.spawn_library_load(path, kind);
    }

    /// Fetch a node's children off-thread, replying with its path.
    pub(crate) fn spawn_library_load(&self, path: Vec<usize>, kind: LibKind) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result: Result<Vec<LibKind>> = match &kind {
                LibKind::LikedMusic => api
                    .liked_songs()
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::LibrarySongs => api
                    .library_songs()
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::PlaylistsFolder => api
                    .library_playlists()
                    .await
                    .map(|p| p.into_iter().map(LibKind::Playlist).collect()),
                LibKind::ArtistsFolder => api
                    .library_artists()
                    .await
                    .map(|a| a.into_iter().map(LibKind::Artist).collect()),
                LibKind::AlbumsFolder => api
                    .library_albums()
                    .await
                    .map(|a| a.into_iter().map(LibKind::Album).collect()),
                LibKind::Playlist(p) => api
                    .playlist_tracks(&p.id)
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::Artist(a) => api
                    .artist_albums(&a.channel_id)
                    .await
                    .map(|al| al.into_iter().map(LibKind::Album).collect()),
                LibKind::Album(a) => api
                    .album_tracks(&a.id)
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::Song(_) => Ok(Vec::new()),
            };
            let msg = match result {
                Ok(children) => AppMsg::LibChildren { path, children },
                Err(e) => AppMsg::LibFailed {
                    path,
                    error: format!("load failed: {e}"),
                },
            };
            let _ = tx.send(msg);
        });
    }

    // --- search / radio / lyrics / likes ---------------------------------

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

    fn radio_from_current(&mut self) {
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

    fn like_current(&mut self) {
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
    use crate::api::{AlbumRef, ArtistRef};

    fn song(title: &str) -> LibKind {
        LibKind::Song(Track {
            video_id: title.to_lowercase(),
            title: title.into(),
            artist: "A".into(),
            duration_text: "3:00".into(),
            ..Default::default()
        })
    }

    /// Build a tree without the App, to test flattening in isolation.
    fn flatten(nodes: &[LibNode]) -> Vec<LibRow> {
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
    fn collapsed_nodes_hide_their_children() {
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
    fn expanding_reveals_children_with_increasing_depth() {
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
        assert_eq!(rows.len(), 5, "{:?}", rows.iter().map(|r| &r.label).collect::<Vec<_>>());
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].depth, 2);
        assert_eq!(rows[3].depth, 3);
        assert!(rows[3].is_song);
        assert_eq!(rows[3].path, vec![0, 0, 0, 0]);
        assert_eq!(rows[4].path, vec![0, 0, 0, 1]);
    }

    #[test]
    fn paths_address_the_right_node_at_every_depth() {
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
    fn songs_are_leaves_and_never_show_a_disclosure_marker() {
        let rows = flatten(&[LibNode::new(song("Solo"))]);
        assert_eq!(rows[0].marker, " ");
        assert!(rows[0].is_song);
        // A song counts as already loaded, so it is never fetched.
        assert!(LibNode::new(song("Solo")).loaded);
    }

    #[test]
    fn loading_nodes_show_a_progress_marker() {
        let mut n = LibNode::new(LibKind::ArtistsFolder);
        n.loading = true;
        let rows = flatten(&[n]);
        assert_eq!(rows[0].marker, "\u{22ef}");
    }

    #[test]
    fn labels_and_sublabels_describe_each_kind() {
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
