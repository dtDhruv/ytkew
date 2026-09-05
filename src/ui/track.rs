//! The track view -- cover, metadata, visualizer and progress bar.

use crate::app::App;
use crate::ui::widgets::{
    draw_cover, draw_cover_placeholder, draw_metadata, draw_progress, draw_visualizer,
};
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

use super::layout::*;
use super::lists::{draw_library, draw_up_next};
use super::views::reserve_cells;
use super::views::{centered_row, panel};

/// kew's portrait track layout: cover, gap, metadata, time, lyric, then the
/// visualizer with the progress bar beneath it.
pub(super) fn draw_track(f: &mut Frame, area: Rect, app: &mut App) {
    let (player, side) = track_panes(area, app);
    // Input needs to know whether there is a side pane to steer the selection
    // keys at; the renderer is what knows the terminal is wide enough.
    app.side_pane_open = side.is_some();
    if let Some(side) = side {
        match app.cfg.side_pane {
            crate::config::SidePane::Library => draw_library(f, side, app),
            _ => draw_up_next(f, side, app),
        }
    }

    let (pos, total) = app.queue.human_position();
    let title = if app.player_state.paused && app.queue.current().is_some() {
        "paused"
    } else if app.queue.current().is_some() {
        "now playing"
    } else {
        "track"
    };
    let area = panel(
        f,
        player,
        title,
        (total > 0).then_some((pos.saturating_sub(1), total)),
        app,
    );
    let Some(track) = app.queue.current() else {
        draw_splash(f, area, app);
        return;
    };

    let l = track_layout(area, app);

    if let Some(rect) = l.cover {
        if app.graphics_active() {
            // The image is written straight to the terminal after the frame,
            // so tell ratatui to leave these cells alone rather than painting
            // over the graphics.
            reserve_cells(f, rect);
        } else {
            match &app.cover {
                Some(c) => draw_cover(f, rect, c),
                None => draw_cover_placeholder(f, rect, &app.palette),
            }
        }
    }

    draw_metadata(f, l.meta, track, &app.palette);

    if let Some(viz) = l.viz {
        // Aim for a fine spectrum rather than a few chunky columns: wide
        // panes get narrower bars instead of wider ones.
        let bar_w = app
            .cfg
            .visualizer_bar_width
            .max(1)
            .min((viz.width / 40).max(1));
        let bars = (viz.width / bar_w) as usize;
        let values = app.visual.bars(bars);
        draw_visualizer(
            f,
            viz,
            &values,
            &app.palette,
            app.cfg.visualizer_mode,
            bar_w,
        );
    }

    let track_bounds = draw_progress(
        f,
        l.progress,
        app.player_state.time_pos,
        effective_duration(app),
        &app.palette,
    );
    app.hits.progress = Some(l.progress);
    app.hits.progress_track = track_bounds;
}

/// Nothing playing.
pub(super) fn draw_splash(f: &mut Frame, area: Rect, app: &App) {
    let hint = if app.api.is_authenticated() {
        "2 for your library  ·  / to search  ·  esc for the menu"
    } else {
        "/ to search  ·  `ytkew --auth cookie` for your library"
    };
    Paragraph::new(hint)
        .alignment(Alignment::Center)
        .render(centered_row(area), f.buffer_mut());
}

/// Prefer mpv's duration (authoritative) but fall back to the API's, so the
/// progress bar is populated during the seconds before the stream resolves.
pub(super) fn effective_duration(app: &App) -> f64 {
    if app.player_state.duration > 0.0 {
        app.player_state.duration
    } else {
        app.queue.current().and_then(|t| t.duration).unwrap_or(0.0)
    }
}
