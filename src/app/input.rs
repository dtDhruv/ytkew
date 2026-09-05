//! Turning key presses and mouse events into actions on the app.

use crate::config::{Action, VisualizerMode};
use crate::model::Track;
use crate::queue::RepeatMode;
use crate::ui::View;
use anyhow::Result;

use super::*;

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
    /// Library columns: the area, the entry its first row shows, and which
    /// depth it draws. Replaces `list` while the miller view is up, since one
    /// flat index cannot address several columns.
    pub lib_columns: Vec<(ratatui::layout::Rect, usize, usize)>,
}

impl App {
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
                        self.announce_seek(d * pct);
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

    pub(crate) fn tab_at(&self, col: u16, row: u16) -> Option<View> {
        self.hits
            .tabs
            .iter()
            .find(|(r, _)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
            .map(|(_, v)| *v)
    }

    pub(crate) fn point_in_list(&self, col: u16, row: u16) -> bool {
        self.hits.list.is_some_and(|(r, _)| {
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        })
    }

    /// Item index under the pointer, if it is over a list row.
    pub(crate) fn row_at(&self, col: u16, row: u16) -> Option<usize> {
        if !self.hits.lib_columns.is_empty() {
            return self.library_row_at(col, row);
        }
        let (r, start) = self.hits.list?;
        if col < r.x || col >= r.x + r.width || row < r.y || row >= r.y + r.height {
            return None;
        }
        let index = start + (row - r.y) as usize;
        (index < self.list_len()).then_some(index)
    }

    /// Resolve a click in the column view back to a row in the stacked list,
    /// which is what `library_sel` indexes.
    fn library_row_at(&self, col: u16, row: u16) -> Option<usize> {
        let (rect, start, depth) = self.hits.lib_columns.iter().copied().find(|(r, _, _)| {
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        })?;
        let columns = self.library_columns();
        let column = columns.get(depth)?;
        let path = column.rows.get(start + (row - rect.y) as usize)?;
        self.library_row_index(path)
    }

    /// Fraction along a progress bar that was clicked, if any.
    pub(crate) fn progress_at(&self, col: u16, row: u16) -> Option<f64> {
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

    pub(crate) fn selection(&self) -> Option<usize> {
        match self.view {
            View::Queue => Some(self.queue_sel),
            View::Library => Some(self.library_sel),
            View::Search => Some(self.search_sel),
            _ => None,
        }
    }

    // --- selection --------------------------------------------------------

    /// The pane the selection keys act on.
    ///
    /// On the track view that is the side pane when one is showing: the
    /// player column has nothing to select, so j/k and enter would otherwise
    /// do nothing at all on the view most people leave open.
    pub(crate) fn active_list(&self) -> View {
        if self.view == View::Track && self.side_pane_open {
            return match self.cfg.side_pane {
                crate::config::SidePane::Library => View::Library,
                _ => View::Queue,
            };
        }
        self.view
    }

    pub(crate) fn list_len(&self) -> usize {
        match self.active_list() {
            View::Queue => self.queue.len(),
            View::Library => self.library_rows.len(),
            View::Search => self.search_results.len(),
            _ => 0,
        }
    }

    /// True while the library pane is showing columns and is the pane the
    /// keys act on.
    pub(crate) fn in_library_columns(&self) -> bool {
        self.view == View::Library && self.library_columns_open
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.view == View::Lyrics {
            self.lyrics_scroll = self.lyrics_scroll.saturating_add_signed(delta as i16);
            return;
        }
        // In columns, up and down stay within one level. Walking into
        // children would slide the cursor out of the list being read.
        if self.in_library_columns() {
            self.library_move_sibling(delta);
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
        if self.in_library_columns() {
            self.library_edge_sibling(to > 0);
            return;
        }
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
        match self.active_list() {
            View::Queue => {
                if self.queue_sel < self.queue.len() {
                    let i = self.queue_sel;
                    if self.queue.jump_to(i).is_some() {
                        self.start_current();
                    }
                }
            }
            View::Search => self.activate_search_hit(jump),
            View::Library => self.activate_library_row(jump),
            _ => {}
        }
    }

    /// Play a single track picked from outside the running queue.
    ///
    /// While a playlist is on, it slots in as the next track and the playlist
    /// resumes after it -- YouTube Music's behaviour, and the only one that
    /// does not make you rebuild your queue to hear one song. With no
    /// playlist context it just plays.
    pub(crate) fn play_one_off(&mut self, track: Track) {
        let in_playlist = matches!(self.queue_origin, Some(QueueOrigin::Library(_)))
            && self.queue.current().is_some();
        if !in_playlist {
            self.enqueue_track(track, true);
            if self.queue_origin.is_none() {
                self.queue_origin = Some(QueueOrigin::Search);
            }
            return;
        }
        let at = self.queue.insert_next(track);
        self.queue.jump_to(at);
        self.start_current();
    }

    /// Append to the end of the queue without disturbing what is playing.
    ///
    /// Enter plays things, so building a queue up front needs a key of its
    /// own.
    pub(crate) fn add_selection_to_queue(&mut self) {
        let tracks: Vec<Track> = match self.active_list() {
            View::Search => match self.search_results.get(self.search_sel) {
                Some(hit) => hit.track().cloned().into_iter().collect(),
                None => Vec::new(),
            },
            View::Library => {
                let path = self.library_cursor();
                match self.node_at(&path).map(|n| n.kind.clone()) {
                    // A container adds everything under it.
                    Some(LibKind::Song(t)) => vec![t],
                    Some(_) => self.songs_under(&path),
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        if tracks.is_empty() {
            self.notify("nothing here to add");
            return;
        }
        let n = tracks.len();
        let was_idle = self.queue.current().is_none();
        self.queue.extend(tracks);
        if was_idle {
            self.queue.jump_to(0);
            self.start_current();
        } else {
            self.resync_prefetch();
            self.notify(if n == 1 {
                "added to queue".to_string()
            } else {
                format!("added {n} tracks")
            });
        }
    }

    /// Append one track. Starts playback if nothing is going, so a first
    /// Enter on a fresh queue still does the obvious thing.
    pub(crate) fn enqueue_track(&mut self, track: Track, jump: bool) {
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

    pub(crate) fn remove_selection(&mut self) {
        if self.view == View::Queue && self.queue_sel < self.queue.len() {
            self.queue.remove(self.queue_sel);
            if self.queue_sel >= self.queue.len() {
                self.queue_sel = self.queue.len().saturating_sub(1);
            }
            self.resync_prefetch();
        }
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
            // Left and right walk the columns while the library is showing
            // them. Every column browser works this way, and these keys have
            // no list to steer in that pane otherwise.
            Action::Prev if self.in_library_columns() => self.library_ascend(),
            Action::Next if self.in_library_columns() => self.library_descend(),
            // In search they step through the filters, which is the only
            // navigation that pane has.
            Action::Prev if self.view == View::Search => self.cycle_search_filter(-1),
            Action::Next if self.view == View::Search => self.cycle_search_filter(1),
            Action::Next => self.next_track().await,
            Action::Prev => self.prev_track(),
            Action::SeekForward => {
                self.player.seek(self.cfg.seek_step)?;
                self.announce_seek(self.player_state.time_pos + self.cfg.seek_step);
            }
            Action::SeekBack => {
                self.player.seek(-self.cfg.seek_step)?;
                self.announce_seek(self.player_state.time_pos - self.cfg.seek_step);
            }
            Action::VolumeUp => {
                let v = self.player.add_volume(self.cfg.volume_step).await?;
                self.notify(format!("volume {}%", v.round() as i64));
            }
            Action::VolumeDown => {
                let v = self.player.add_volume(-self.cfg.volume_step).await?;
                self.notify(if v <= 0.0 {
                    "volume 0% -- muted".to_string()
                } else {
                    format!("volume {}%", v.round() as i64)
                });
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
                self.clear_cover_art();
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
            Action::AddToQueue => self.add_selection_to_queue(),
            Action::ClearQueue => {
                // Stop a still-arriving playlist from refilling what was just
                // cleared.
                self.queue_feed = None;
                self.queue_origin = None;
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
}
