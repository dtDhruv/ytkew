//! The five panes plus lyrics, rendered in kew's arrangement.

use crate::app::App;
use crate::model::fmt_duration;
use crate::config::VisualizerMode;
use crate::ui::widgets::{
    draw_cover, draw_cover_placeholder, draw_metadata, draw_progress, draw_visualizer, elide,
};
use crate::ui::View;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    StatefulWidget, Widget,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// Geometry for the track view, computed once so the renderer and the sixel
/// painter can never disagree.
///
/// Everything lives in one centred column the width of the cover. kew does
/// the same (its `calc_indent_normal`): with the cover centred but the text
/// hard against the left margin, a wide terminal leaves a large dead gap.
pub struct TrackLayout {
    pub cover: Option<Rect>,
    pub meta: Rect,
    pub viz: Option<Rect>,
    pub progress: Rect,
}

/// Title, artist, album.
const META_ROWS: u16 = 3;
const PROGRESS_ROWS: u16 = 1;
/// A cover that eats the whole pane looks unbalanced, and on a tall terminal
/// it dwarfs everything else, so cap its share of the height.
const COVER_MAX_ROWS: u16 = 18;
const COVER_MAX_HEIGHT_FRACTION: f32 = 0.55;
/// Below this the art is too small to be worth the rows it costs, so the
/// space goes to the text and visualizer instead.
const COVER_MIN_ROWS: u16 = 6;
/// The column never gets narrower than this, or titles and the progress bar
/// truncate on a short terminal where the cover is small. At the usual cover
/// size the cover is exactly this wide, so the two coincide.
const MIN_COL_WIDTH: u16 = 36;

pub fn track_layout(area: Rect, app: &App) -> TrackLayout {
    let viz_h = if matches!(app.cfg.visualizer_mode, VisualizerMode::Off) {
        0
    } else {
        app.cfg.visualizer_height
    };
    let draws_cover = app.cover_visible && app.cfg.cover_mode.draws_anything();
    compute_track_layout(area, app.cell_px, viz_h, draws_cover)
}

/// The geometry, free of `App` so it can be tested directly.
pub fn compute_track_layout(
    area: Rect,
    cell_px: (u16, u16),
    visualizer_height: u16,
    draws_cover: bool,
) -> TrackLayout {
    let viz_h = visualizer_height.min(area.height / 3);
    // Fixed rows: gap after the cover, metadata, gap, visualizer, bar.
    let fixed = 1 + META_ROWS + 1 + viz_h + PROGRESS_ROWS;
    let spare = area.height.saturating_sub(fixed);
    let cover_h = if draws_cover {
        let h = spare
            .min(COVER_MAX_ROWS)
            .min((area.height as f32 * COVER_MAX_HEIGHT_FRACTION) as u16);
        if h < COVER_MIN_ROWS {
            0
        } else {
            h
        }
    } else {
        0
    };

    // Keep the art square: a cell is taller than it is wide, so a square
    // image needs cell_h/cell_w times as many columns as rows.
    let (cw, ch) = cell_px;
    let ratio = (ch as f32 / cw.max(1) as f32).max(1.0);
    let cover_w = if cover_h > 0 {
        ((cover_h as f32 * ratio).round() as u16)
            .min(area.width)
            .max(1)
    } else {
        0
    };

    // The column is the cover's width, so text, visualizer and progress bar
    // line up with the art's left edge -- kew ties its visualizer width to
    // the cover the same way. A floor keeps the text readable when the cover
    // is small; above that floor the two are equal and the alignment is exact.
    let col_w = cover_w
        .max(if cover_w > 0 {
            MIN_COL_WIDTH
        } else {
            (area.width * 2 / 3).max(24)
        })
        .min(area.width);
    let x = area.x + (area.width.saturating_sub(col_w)) / 2;

    // Centre the whole block vertically so leftover space is shared top and
    // bottom instead of pooling under the text.
    let content_h = cover_h + fixed;
    let mut y = area.y + area.height.saturating_sub(content_h) / 2;

    let cover = if cover_h > 0 {
        // Left-aligned with the rest of the column. Keeping one shared left
        // edge matters more than centring the art within the column, and on a
        // normal-sized terminal the cover fills the column anyway.
        let r = Rect::new(x, y, cover_w, cover_h);
        y += cover_h + 1;
        Some(r)
    } else {
        None
    };

    let meta = Rect::new(x, y, col_w, META_ROWS);
    y += META_ROWS + 1;
    let viz = if viz_h > 0 {
        let r = Rect::new(x, y, col_w, viz_h);
        y += viz_h;
        Some(r)
    } else {
        None
    };
    let progress = Rect::new(x, y, col_w, PROGRESS_ROWS);

    TrackLayout {
        cover,
        meta,
        viz,
        progress,
    }
}

/// The track panel's content area. Both the renderer and the graphics
/// placement go through this, so the border inset is applied exactly once.
pub fn track_inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

/// Where the cover goes, for the sixel/kitty painter.
pub fn cover_rect(area: Rect, app: &App) -> Option<Rect> {
    track_layout(track_inner(area), app).cover
}

/// Scroll window that keeps the selection visible and roughly centred.
pub fn window(sel: usize, len: usize, height: usize) -> (usize, usize) {
    if len == 0 || height == 0 {
        return (0, 0);
    }
    if len <= height {
        return (0, len);
    }
    let half = height / 2;
    let start = sel.saturating_sub(half).min(len - height);
    (start, start + height)
}

/// Rows of chrome above the content: the tab strip and its rule.
pub const TABBAR_ROWS: u16 = 2;

/// Rows of chrome below: the now-playing bar plus the hint line. The track
/// view is itself the player, so it does not repeat the bar.
pub fn footer_rows(app: &App) -> u16 {
    if app.view == View::Track || app.queue.current().is_none() {
        1
    } else {
        2
    }
}

/// The region between the tab strip and the footer.
pub fn body_rect(area: Rect) -> Rect {
    // Callers that only have the frame use the two-row footer, which is the
    // larger of the two; a one-row difference does not move the cover.
    Rect::new(
        area.x,
        area.y + TABBAR_ROWS,
        area.width,
        area.height.saturating_sub(TABBAR_ROWS + 2),
    )
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.height < 3 || area.width < 20 {
        Paragraph::new("terminal too small").render(area, f.buffer_mut());
        return;
    }
    // Regions are re-recorded every frame so clicks always match what is on
    // screen right now.
    app.hits = crate::app::HitRegions::default();
    // A cover was just removed and its cells blanked on the terminal; mirror
    // that in the buffer so the diff does not think they still hold art.
    if let Some(stale) = app.take_stale_cover() {
        let buf = f.buffer_mut();
        for y in stale.y..stale.y.saturating_add(stale.height) {
            for x in stale.x..stale.x.saturating_add(stale.width) {
                if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                    cell.reset();
                }
            }
        }
    }
    let foot_h = footer_rows(app);
    let tabbar = Rect::new(area.x, area.y, area.width, TABBAR_ROWS);
    let body = Rect::new(
        area.x,
        area.y + TABBAR_ROWS,
        area.width,
        area.height.saturating_sub(TABBAR_ROWS + foot_h),
    );
    let footer = Rect::new(
        area.x,
        area.y + area.height - foot_h,
        area.width,
        foot_h,
    );

    draw_tabbar(f, tabbar, app);
    match app.view {
        View::Track => draw_track(f, body, app),
        View::Queue => draw_queue(f, body, app),
        View::Library => draw_library(f, body, app),
        View::Search => draw_search(f, body, app),
        View::Help => {
            // Draw the track view behind, then float the keys over it, so the
            // help reads as an overlay rather than a page you navigated to.
            draw_track(f, body, app);
            draw_help(f, body, app);
        }
        View::Lyrics => draw_lyrics(f, body, app),
    }
    // The menu floats above everything, btop-style.
    if app.menu_open {
        draw_menu(f, body, app);
    }

    // Everywhere but the track view, keep the transport visible.
    if foot_h == 2 {
        let bar = Rect::new(footer.x, footer.y, footer.width, 1);
        draw_player_bar(f, bar, app);
        draw_footer(
            f,
            Rect::new(footer.x, footer.y + 1, footer.width, 1),
            app,
        );
    } else {
        draw_footer(f, footer, app);
    }
}

/// The overlay: btop's three block-letter entries, or the options pane once
/// Options is chosen.
fn draw_menu(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::MenuScreen;
    match app.menu_screen {
        MenuScreen::Main => draw_menu_main(f, area, app),
        MenuScreen::Options => draw_options(f, area, app),
    }
}

fn draw_menu_main(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::{App as A, MENU_ITEMS};
    use crate::ui::bigtext;

    let accent = app.palette.accent().to_color();
    let faint = app.palette.dark.to_color();

    let words: Vec<&str> = MENU_ITEMS.iter().map(|i| A::menu_word(*i)).collect();
    let item_w = words.iter().map(|w| bigtext::width(w)).max().unwrap_or(12);
    // The banner is the widest thing here, but drop it on a pane too small to
    // hold it rather than clipping the logo.
    let show_banner = area.width as usize >= crate::ui::banner::width() + 6
        && area.height as usize >= crate::ui::banner::ROWS + words.len() * 3 + 6;
    let banner_w = if show_banner {
        crate::ui::banner::width()
    } else {
        0
    };
    let grid_w = item_w.max(banner_w);
    let banner_h = if show_banner {
        crate::ui::banner::ROWS + 1
    } else {
        0
    };
    // Border, plus a blank row inside it top and bottom, plus side padding --
    // the art needs room around it or the frame crowds the logo.
    const PAD_V: usize = 2;
    const PAD_H: u16 = 8;
    let h = ((banner_h + words.len() * 3 + words.len() - 1 + 2 + PAD_V) as u16).min(area.height);
    let w = (grid_w as u16 + PAD_H + 2).min(area.width);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    Clear.render(popup, f.buffer_mut());
    let inner = panel(f, popup, "menu", None, app);
    // Inset the content by the padding rows, keeping it centred.
    let inner = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(2),
    );

    let mut lines: Vec<Line> = if show_banner {
        let mut b = banner_lines(app, grid_w);
        b.push(Line::default());
        b
    } else {
        Vec::new()
    };
    for (i, word) in words.iter().enumerate() {
        let selected = i == app.menu_sel;
        for row in bigtext::render(word, selected) {
            // Centred under the banner, which is wider than any item.
            lines.push(Line::from(Span::styled(
                centre(&row, grid_w),
                if selected {
                    Style::default().fg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(faint)
                },
            )));
        }
        if i + 1 < words.len() {
            lines.push(Line::default());
        }
    }
    let x = inner.x + (inner.width.saturating_sub(grid_w as u16)) / 2;
    Paragraph::new(lines).render(
        Rect::new(x, inner.y, grid_w as u16, inner.height),
        f.buffer_mut(),
    );
}

/// The wordmark banner, centred and shaded down the ramp.
fn banner_lines(_app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::ui::banner;
    let shades = banner::shades();
    banner::rows()
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            Line::from(Span::styled(
                centre(&row, width),
                Style::default()
                    .fg(shades[i].to_color())
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

/// The options pane. Each setting is two rows -- its name, then its value --
/// with arrows beside the value of the selected row, and that row's
/// description below the list. This is btop's options layout.
fn draw_options(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::SETTINGS;

    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();

    const BODY: u16 = 40;
    let w = (BODY + 4).min(area.width);
    // Two rows per setting, a blank line, two of description, two of border.
    let h = ((SETTINGS.len() as u16 * 2) + 5).min(area.height);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    Clear.render(popup, f.buffer_mut());
    let inner = panel(f, popup, "options", None, app);
    if inner.width < 8 || inner.height < 4 {
        return;
    }
    let width = inner.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    for (i, setting) in SETTINGS.iter().enumerate() {
        let selected = i == app.option_sel;
        // Name row.
        lines.push(Line::from(Span::styled(
            centre(setting.label(), width),
            if selected {
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(dim).add_modifier(Modifier::BOLD)
            },
        )));
        // Value row, with arrows only where they can be used.
        let value = app.setting_value(*setting);
        if selected {
            // One column for each arrow, so the three spans total exactly the
            // row width and the arrows sit hard against the edges.
            let inner_w = width.saturating_sub(2);
            lines.push(Line::from(vec![
                Span::styled("←", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
                Span::styled(centre(&value, inner_w), Style::default().fg(accent)),
                Span::styled("→", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                centre(&value, width),
                Style::default().fg(faint),
            )));
        }
    }
    lines.push(Line::default());

    // Description of the highlighted setting, wrapped to two lines.
    let desc = SETTINGS[app.option_sel.min(SETTINGS.len() - 1)].description();
    for chunk in wrap(desc, width, 2) {
        lines.push(Line::from(Span::styled(
            chunk,
            Style::default().fg(dim).add_modifier(Modifier::ITALIC),
        )));
    }
    Paragraph::new(lines).render(inner, f.buffer_mut());
}

/// Centre text in exactly `w` columns, padded on both sides so a highlighted
/// row spans the full width and the right-hand arrow sits at the edge.
fn centre(s: &str, w: usize) -> String {
    let t = elide(s, w as u16);
    let used = t.width();
    let left = (w - used) / 2;
    let right = w - used - left;
    let mut out = " ".repeat(left);
    out.push_str(&t);
    out.push_str(&" ".repeat(right));
    out
}

/// Greedy wrap to at most `max_lines` lines of `w` columns.
fn wrap(text: &str, w: usize, max_lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if candidate.width() > w && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            if out.len() == max_lines {
                // Mark that more was cut rather than ending mid-sentence.
                if let Some(last) = out.last_mut() {
                    if last.width() < w {
                        last.push('…');
                    }
                }
                return out;
            }
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() && out.len() < max_lines {
        out.push(line);
    }
    out
}

/// Browser-style tab strip: a rule under the whole width that breaks around
/// the active tab, so the tab reads as joined to the content below it.
fn draw_tabbar(f: &mut Frame, area: Rect, app: &mut App) {
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();

    let mut tabs = vec![View::Queue, View::Library, View::Track, View::Search];
    if matches!(app.view, View::Help | View::Lyrics) {
        tabs.push(app.view);
    }

    let mut spans = vec![Span::styled(
        " ytkew ",
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )];
    let mut col: u16 = 7;
    // Where the active tab starts and ends, for breaking the rule.
    let mut active: Option<(u16, u16)> = None;
    let mut hit_tabs: Vec<(Rect, View)> = Vec::new();

    for (i, v) in tabs.iter().enumerate() {
        let label = format!("  {} {}  ", i + 1, v.title());
        let w = label.chars().count() as u16;
        let selected = app.view == *v;
        if selected {
            active = Some((col, col + w));
        }
        hit_tabs.push((Rect::new(area.x + col, area.y, w, 1), *v));
        spans.push(Span::styled(
            label,
            if selected {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(dim)
            },
        ));
        col += w;
    }

    // Right-hand status, as before.
    let mut right = status_spans(app, accent, dim, faint);
    let left_w = col as usize;
    let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
    if left_w + right_w < area.width as usize {
        spans.push(Span::raw(
            " ".repeat(area.width as usize - left_w - right_w),
        ));
        spans.append(&mut right);
    }
    Paragraph::new(Line::from(spans)).render(
        Rect::new(area.x, area.y, area.width, 1),
        f.buffer_mut(),
    );

    app.hits.tabs = hit_tabs;

    // The rule, with a gap where the active tab sits.
    let (a, b) = active.unwrap_or((0, 0));
    let rule = tab_rule(area.width as usize, a as usize, b as usize);
    Paragraph::new(Line::from(Span::styled(
        rule,
        Style::default().fg(faint),
    )))
    .render(
        Rect::new(area.x, area.y + 1, area.width, 1),
        f.buffer_mut(),
    );
}

/// The rule under the tab strip, broken around the active tab so the tab
/// reads as joined to the content below it -- the same trick a browser's tab
/// bar uses.
fn tab_rule(width: usize, active_start: usize, active_end: usize) -> String {
    let a = active_start.min(width);
    let b = active_end.min(width);
    (0..width)
        .map(|x| {
            if a < b && x + 1 == a {
                '\u{2518}' // turn up into the gap
            } else if a < b && x == b {
                '\u{2514}' // turn back down out of it
            } else if a < b && x >= a && x < b {
                ' '
            } else {
                '\u{2500}'
            }
        })
        .collect()
}

/// Transport indicators for the top-right corner.
fn status_spans<'a>(
    app: &App,
    accent: ratatui::style::Color,
    dim: ratatui::style::Color,
    faint: ratatui::style::Color,
) -> Vec<Span<'a>> {
    let mut right: Vec<Span> = Vec::new();
    if app.api.is_offline() {
        right.push(Span::styled("offline  ", Style::default().fg(faint)));
    } else if !app.api.is_authenticated() {
        right.push(Span::styled("not signed in  ", Style::default().fg(faint)));
    }
    if !app.queue.is_empty() {
        let (pos, total) = app.queue.human_position();
        right.push(Span::styled(
            format!("{pos}/{total}  "),
            Style::default().fg(dim),
        ));
    }
    if app.queue.shuffle {
        right.push(Span::styled("⇄ ", Style::default().fg(accent)));
    }
    match app.queue.repeat {
        crate::queue::RepeatMode::All => {
            right.push(Span::styled("↻ ", Style::default().fg(accent)))
        }
        crate::queue::RepeatMode::One => {
            right.push(Span::styled("↻1 ", Style::default().fg(accent)))
        }
        crate::queue::RepeatMode::Off => {}
    }
    right.push(Span::styled(
        format!("vol {}% ", app.player_state.volume.round() as i64),
        Style::default().fg(dim),
    ));
    right
}

/// The always-visible now-playing bar, like the one every graphical player
/// keeps pinned to the bottom: state glyph, track, and an inline progress bar.
fn draw_player_bar(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(track) = app.queue.current().cloned() else {
        return;
    };
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();

    let glyph = if app.player_state.buffering {
        "⋯"
    } else if app.player_state.paused {
        "⏸"
    } else {
        "▶"
    };
    let elapsed = fmt_duration(app.player_state.time_pos);
    let total = {
        let d = effective_duration(app);
        if d > 0.0 {
            fmt_duration(d)
        } else {
            "--:--".into()
        }
    };
    let times = format!("{elapsed} / {total}");

    let label = if track.artist.is_empty() {
        track.title.clone()
    } else {
        format!("{} — {}", track.title, track.artist)
    };

    // Left: chip, state glyph, track. Right: bar then times. The label takes
    // what it needs up to a share of the width, and the bar absorbs the rest
    // so the two halves stay visually connected instead of drifting apart.
    let width = area.width as usize;
    // " " + chip(2) + " x " + "  " before the times.
    let fixed = 1 + 2 + 3 + 2 + times.chars().count();
    let avail = width.saturating_sub(fixed);
    let label_w = label.width().min(avail * 3 / 5);
    let bar_w = avail.saturating_sub(label_w + 2).clamp(0, 48);
    // Any leftover goes between the label and the bar.
    let gap = avail.saturating_sub(label_w + bar_w);

    let frac = {
        let d = effective_duration(app);
        if d > 0.0 {
            (app.player_state.time_pos / d).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let filled = ((frac * bar_w as f64).round() as usize).min(bar_w.saturating_sub(1));

    let mut spans = vec![
        Span::styled(" ", Style::default()),
        Span::styled("▐▌", Style::default().fg(accent)),
        Span::styled(format!(" {glyph} "), Style::default().fg(accent)),
        Span::styled(
            fit(&label, label_w),
            Style::default().fg(dim).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
    ];
    if bar_w > 2 {
        spans.push(Span::styled(
            "━".repeat(filled),
            Style::default().fg(accent),
        ));
        spans.push(Span::styled("●", Style::default().fg(accent)));
        spans.push(Span::styled(
            "━".repeat(bar_w - filled - 1),
            Style::default().fg(faint),
        ));
    }
    spans.push(Span::styled(
        format!("  {times}"),
        Style::default().fg(faint),
    ));
    Paragraph::new(Line::from(spans)).render(area, f.buffer_mut());

    // The mini bar is seekable too, so the transport works from any view.
    if bar_w > 2 {
        let track_x = area.x + (1 + 2 + 3 + label_w + gap) as u16;
        app.hits.progress = Some(area);
        app.hits.progress_track = Some((track_x, bar_w as u16));
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let dim = app.palette.secondary().to_color();
    let accent = app.palette.accent().to_color();

    // A transient message always wins; it is the reason the user looked down.
    if let Some(msg) = app.status_text() {
        Paragraph::new(Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(accent),
        )))
        .render(area, f.buffer_mut());
        return;
    }
    if app.cfg.hide_help {
        return;
    }

    // Hints for the current pane rather than one static line listing
    // everything, most of which does not apply where you are.
    let hints: &[(&str, &str)] = match app.view {
        View::Track => &[
            ("space", "play"),
            ("h/l", "skip"),
            ("a/d", "seek"),
            ("+/-", "vol"),
            ("s", "shuffle"),
            ("r", "repeat"),
            ("m", "lyrics"),
            ("esc", "menu"),
        ],
        View::Queue => &[
            ("enter", "play"),
            ("j/k", "move"),
            ("f/g", "reorder"),
            ("del", "remove"),
            ("bksp", "clear all"),
            ("esc", "menu"),
        ],
        View::Library => &[
            ("enter", "open"),
            ("alt+enter", "play all"),
            ("j/k", "move"),
            ("esc", "menu"),
        ],
        View::Search => &[
            ("enter", "search"),
            ("i", "edit"),
            ("j/k", "move"),
            ("P", "play all"),
        ],
        View::Lyrics => &[("j/k", "scroll"), ("tab", "back")],
        View::Help => &[("tab", "back")],
    };

    let mut spans = vec![Span::raw(" ")];
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(app.palette.dark.to_color())));
        }
        spans.push(Span::styled(*key, Style::default().fg(accent)));
        spans.push(Span::styled(format!(" {label}"), Style::default().fg(dim)));
    }
    spans.push(Span::styled(
        "   F6 help",
        Style::default().fg(app.palette.dark.to_color()),
    ));
    Paragraph::new(Line::from(spans)).render(area, f.buffer_mut());
}

/// kew's portrait track layout: cover, gap, metadata, time, lyric, then the
/// visualizer with the progress bar beneath it.
fn draw_track(f: &mut Frame, area: Rect, app: &mut App) {
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
        area,
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
fn draw_splash(f: &mut Frame, area: Rect, app: &App) {
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
fn effective_duration(app: &App) -> f64 {
    if app.player_state.duration > 0.0 {
        app.player_state.duration
    } else {
        app.queue.current().and_then(|t| t.duration).unwrap_or(0.0)
    }
}

fn draw_queue(f: &mut Frame, area: Rect, app: &mut App) {
    let total_all = app.queue.len();
    let area = panel(
        f,
        area,
        "queue",
        (total_all > 0).then_some((app.queue_sel, total_all)),
        app,
    );
    if app.queue.is_empty() {
        return centered_message(f, area, "queue is empty — / to search, 2 for your library");
    }
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();
    let playing = app.queue.current_index();
    let total = app.queue.len();
    let (start, end) = window(app.queue_sel, total, area.height as usize);

    // Column widths: marker, index, duration and scrollbar are fixed, and
    // artist/title share what is left so the columns line up down the list.
    let idx_w = total.to_string().len();
    let bar_w = if total > area.height as usize { 1 } else { 0 };
    // marker(2) + index + two separator spaces + duration(6) + scrollbar.
    let fixed = 2 + idx_w + 2 + 6 + bar_w;
    let _ = &fixed;
    let flex = (area.width as usize).saturating_sub(fixed);
    let artist_w = (flex * 2 / 5).min(flex);
    let title_w = flex - artist_w;

    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let t = &app.queue.tracks()[i];
            let is_playing = playing == Some(i);
            let is_sel = i == app.queue_sel;
            let marker = if is_playing { "▶ " } else { "  " };
            let row = format!(
                "{marker}{} {} {}{}",
                rfit(&(i + 1).to_string(), idx_w),
                fit(&t.artist, artist_w),
                fit(&t.title, title_w),
                rfit(&t.duration_text, 6),
            );
            let base = match (is_playing, is_sel) {
                (true, _) => Style::default().fg(accent).add_modifier(Modifier::BOLD),
                (false, true) => Style::default().fg(dim),
                _ => Style::default().fg(faint),
            };
            let style = if is_sel {
                base.add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            Line::from(Span::styled(elide(&row, area.width - bar_w as u16), style))
        })
        .collect();
    Paragraph::new(lines).render(area, f.buffer_mut());

    if bar_w == 1 {
        scrollbar(f, area, app.queue_sel, total, app);
    }
    app.hits.list = Some((area, start));
}

fn draw_library(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.library_rows.len();
    let area = panel(
        f,
        area,
        "library",
        (n > 0).then_some((app.library_sel, n)),
        app,
    );
    let rows = &app.library_rows;
    if rows.is_empty() {
        let msg = if app.api.is_authenticated() {
            "library is empty"
        } else {
            "sign in to see your library: `ytkew --auth cookie`"
        };
        return centered_message(f, area, msg);
    }
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();
    let total = rows.len();
    let (start, end) = window(app.library_sel, total, area.height as usize);
    let bar_w = if total > area.height as usize { 1 } else { 0 };
    let width = area.width.saturating_sub(bar_w as u16);

    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let row = &rows[i];
            let indent = "  ".repeat(row.depth);
            let sub = if row.sublabel.is_empty() {
                String::new()
            } else {
                format!("  {}", row.sublabel)
            };
            let text = format!("{indent}{} {}{sub}", row.marker, row.label);
            // Containers carry the accent, leaves recede, so the hierarchy
            // reads at a glance rather than by counting indents.
            let base = if row.is_song {
                Style::default().fg(faint)
            } else if row.depth == 0 {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(dim)
            };
            let style = if i == app.library_sel {
                base.add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            Line::from(Span::styled(fit(&text, width as usize), style))
        })
        .collect();
    Paragraph::new(lines).render(area, f.buffer_mut());

    if bar_w == 1 {
        scrollbar(f, area, app.library_sel, total, app);
    }
    app.hits.list = Some((area, start));
}

fn draw_search(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.search_results.len();
    let area = panel(f, area, "search", (n > 0).then_some((app.search_sel, n)), app);
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();

    // A block cursor makes it obvious whether the box has focus.
    let cursor = if app.search_editing { "▌" } else { "" };
    let prompt = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled("⌕ ", Style::default().fg(accent)),
        Span::styled(
            format!("{}{cursor}", app.search_input),
            Style::default()
                .fg(accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    Paragraph::new(prompt).render(Rect::new(area.x, area.y, area.width, 1), f.buffer_mut());
    // A rule under the input separates query from results.
    Paragraph::new(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(faint),
    )))
    .render(Rect::new(area.x, area.y + 1, area.width, 1), f.buffer_mut());

    let list = Rect::new(area.x, area.y + 2, area.width, area.height.saturating_sub(2));

    if app.search_editing && !app.suggestions.is_empty() && app.search_results.is_empty() {
        let lines: Vec<Line> = app
            .suggestions
            .iter()
            .take(list.height as usize)
            .map(|sg| {
                Line::from(vec![
                    Span::styled("  ⤷ ", Style::default().fg(faint)),
                    Span::styled(elide(sg, list.width - 4), Style::default().fg(dim)),
                ])
            })
            .collect();
        Paragraph::new(lines).render(list, f.buffer_mut());
        return;
    }
    if app.searching {
        return centered_message(f, list, "searching…");
    }
    if app.search_results.is_empty() {
        return centered_message(f, list, "type a query and press enter");
    }

    let total = app.search_results.len();
    let (start, end) = window(app.search_sel, total, list.height as usize);
    let bar_w = if total > list.height as usize { 1 } else { 0 };
    let idx_w = total.to_string().len();
    let fixed = 2 + idx_w + 2 + 6 + bar_w;
    let flex = (list.width as usize).saturating_sub(fixed);
    let artist_w = (flex * 2 / 5).min(flex);
    let title_w = flex - artist_w;

    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let t = &app.search_results[i];
            let is_sel = i == app.search_sel;
            let row = format!(
                "  {} {} {}{}",
                rfit(&(i + 1).to_string(), idx_w),
                fit(&t.artist, artist_w),
                fit(&t.title, title_w),
                rfit(&t.duration_text, 6),
            );
            let base = if is_sel {
                Style::default().fg(dim)
            } else {
                Style::default().fg(faint)
            };
            let style = if is_sel {
                base.add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            Line::from(Span::styled(elide(&row, list.width - bar_w as u16), style))
        })
        .collect();
    Paragraph::new(lines).render(list, f.buffer_mut());

    if bar_w == 1 {
        scrollbar(f, list, app.search_sel, total, app);
    }
    app.hits.list = Some((list, start));
}

fn draw_lyrics(f: &mut Frame, area: Rect, app: &App) {
    let area = panel(f, area, "lyrics", None, app);
    let Some(text) = &app.lyrics else {
        return centered_message(f, area, "fetching lyrics…");
    };
    let dim = app.palette.secondary().to_color();
    let lines: Vec<Line> = text
        .lines()
        .skip(app.lyrics_scroll as usize)
        .take(area.height as usize)
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(dim))))
        .collect();
    Paragraph::new(lines).render(area, f.buffer_mut());
}

fn draw_help(f: &mut Frame, area: Rect, app: &App) {
    use crate::config::Action::*;
    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let k = |a| app.keymap.keys_for(a).join(", ");

    let entries: Vec<(&str, String)> = vec![
        ("Play / pause", k(PlayPause)),
        ("Previous track", k(Prev)),
        ("Next track", k(Next)),
        ("Seek back", k(SeekBack)),
        ("Seek forward", k(SeekForward)),
        ("Volume up", k(VolumeUp)),
        ("Volume down", k(VolumeDown)),
        ("Shuffle", k(Shuffle)),
        ("Repeat (off/all/one)", k(ToggleRepeat)),
        ("Cycle visualizer", k(CycleVisualizer)),
        ("Show / hide cover art", k(ToggleAscii)),
        ("Menu / options", k(ToggleMenu)),
        ("Like current track", k(ToggleLike)),
        ("Start radio from track", k(StartRadio)),
        ("Add to queue", k(Enqueue)),
        ("Add and play now", k(EnqueueAndPlay)),
        ("Move track up", k(MoveUp)),
        ("Move track down", k(MoveDown)),
        ("Remove from queue", k(Remove)),
        ("Clear queue", k(ClearQueue)),
        ("Queue view", k(ShowQueue)),
        ("Library view", k(ShowLibrary)),
        ("Track view", k(ShowTrack)),
        ("Search view", k(ShowSearch)),
        ("Lyrics", k(ShowLyrics)),
        ("Help", k(ShowHelp)),
        ("Cycle views", k(NextView)),
        ("Quit", k(Quit)),
    ];

    // Two columns, so the whole keymap fits without scrolling.
    const COL: usize = 44;
    let rows = entries.len().div_ceil(2);
    let two_col = area.width as usize >= COL * 2 + 2;
    let cell = |e: &(&str, String)| -> Vec<Span<'static>> {
        vec![
            Span::styled(format!(" {:<23}", e.0), Style::default().fg(dim)),
            Span::styled(
                fit(&e.1, COL - 25),
                Style::default().fg(accent),
            ),
        ]
    };

    let mut lines: Vec<Line> = Vec::new();
    if two_col {
        for r in 0..rows {
            let mut spans = cell(&entries[r]);
            if let Some(right) = entries.get(r + rows) {
                spans.push(Span::styled("│", Style::default().fg(app.palette.dark.to_color())));
                spans.extend(cell(right));
            }
            lines.push(Line::from(spans));
        }
    } else {
        for e in entries.iter() {
            lines.push(Line::from(cell(e)));
        }
    }

    // Centre a box just big enough, and clear behind it.
    let w = (if two_col { COL * 2 + 3 } else { COL + 2 } as u16).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    Clear.render(popup, f.buffer_mut());
    let inner = panel(f, popup, "keys", None, app);
    Paragraph::new(lines).render(inner, f.buffer_mut());
}

/// Mark a region as skipped so the backend emits nothing for it.
fn reserve_cells(f: &mut Frame, area: Rect) {
    let buf = f.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
            }
        }
    }
}

/// A titled panel, in the style binsider uses: the label sits in the top
/// border between pipes, and the position indicator in the bottom border.
/// Returns the inner area to draw content into.
fn panel(f: &mut Frame, area: Rect, title: &str, count: Option<(usize, usize)>, app: &App) -> Rect {
    let accent = app.palette.accent().to_color();
    let faint = app.palette.dark.to_color();
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(faint))
        .title(Line::from(vec![
            Span::styled("| ", Style::default().fg(faint)),
            Span::styled(
                title.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" |", Style::default().fg(faint)),
        ]));
    if let Some((sel, total)) = count {
        block = block.title_bottom(
            Line::from(vec![
                Span::styled("| ", Style::default().fg(faint)),
                Span::styled(
                    format!("{}/{}", sel.saturating_add(1).min(total.max(1)), total),
                    Style::default().fg(accent),
                ),
                Span::styled(" |", Style::default().fg(faint)),
            ])
            .right_aligned(),
        );
    }
    let inner = block.inner(area);
    block.render(area, f.buffer_mut());
    inner
}

/// ratatui's scrollbar, with the arrow caps binsider uses.
fn scrollbar(f: &mut Frame, area: Rect, position: usize, total: usize, app: &App) {
    if total == 0 {
        return;
    }
    let faint = app.palette.dark.to_color();
    StatefulWidget::render(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .style(Style::default().fg(faint)),
        area,
        f.buffer_mut(),
        &mut ScrollbarState::new(total).position(position),
    );
}

/// Fit text to exactly `w` display columns, truncating or padding.
fn fit(s: &str, w: usize) -> String {
    let t = elide(s, w as u16);
    let used = t.width();
    let mut out = t;
    for _ in used..w {
        out.push(' ');
    }
    out
}

/// Right-align text within `w` columns.
fn rfit(s: &str, w: usize) -> String {
    let t = elide(s, w as u16);
    let used = t.width();
    let mut out = String::new();
    for _ in used..w {
        out.push(' ');
    }
    out.push_str(&t);
    out
}

fn centered_message(f: &mut Frame, area: Rect, msg: &str) {
    Paragraph::new(msg)
        .alignment(Alignment::Center)
        .render(centered_row(area), f.buffer_mut());
}

fn centered_row(area: Rect) -> Rect {
    Rect::new(area.x, area.y + area.height / 2, area.width, 1)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn layout(w: u16, h: u16) -> TrackLayout {
        compute_track_layout(Rect::new(0, 0, w, h), (8, 16), 6, true)
    }

    #[test]
    fn every_element_shares_the_covers_left_edge() {
        let l = layout(176, 43);
        let cover = l.cover.expect("cover expected");
        // A single centred column is the whole point: kew indents the text to
        // the cover instead of leaving a gap on a wide terminal.
        assert_eq!(l.meta.x, cover.x);
        assert_eq!(l.progress.x, cover.x);
        assert_eq!(l.viz.unwrap().x, cover.x);
        assert_eq!(l.meta.width, cover.width);
        assert_eq!(l.viz.unwrap().width, cover.width);
    }

    #[test]
    fn the_column_is_horizontally_centred() {
        let l = layout(176, 43);
        let right_gap = 176 - (l.meta.x + l.meta.width);
        assert!(
            l.meta.x.abs_diff(right_gap) <= 1,
            "left {} vs right {right_gap}",
            l.meta.x
        );
    }

    #[test]
    fn a_short_pane_drops_the_cover_rather_than_shrinking_the_text() {
        // A 4-row cover is not worth the rows, and a column that narrow
        // truncates the title and the progress bar.
        let l = layout(70, 16);
        assert!(l.cover.is_none(), "tiny cover should be dropped");
        assert!(
            l.meta.width >= MIN_COL_WIDTH.min(70),
            "column too narrow: {}",
            l.meta.width
        );
    }

    #[test]
    fn the_column_never_falls_below_the_readable_floor() {
        for h in 18..=60u16 {
            let l = layout(120, h);
            assert!(
                l.meta.width >= MIN_COL_WIDTH,
                "height {h}: column {} too narrow",
                l.meta.width
            );
        }
    }

    #[test]
    fn the_cover_is_square_for_the_given_cell_aspect() {
        // 8x16 cells mean a square image needs twice as many columns as rows.
        let l = layout(176, 43);
        let cover = l.cover.unwrap();
        assert_eq!(cover.width, cover.height * 2);
    }

    #[test]
    fn a_tall_pane_caps_the_cover_instead_of_letting_it_dominate() {
        let l = layout(200, 120);
        let cover = l.cover.unwrap();
        assert!(cover.height <= COVER_MAX_ROWS, "got {}", cover.height);
        // And it never takes more than its share of the height.
        assert!(cover.height as f32 <= 120.0 * COVER_MAX_HEIGHT_FRACTION);
    }

    #[test]
    fn elements_stack_in_kews_order_without_overlapping() {
        let l = layout(176, 43);
        let cover = l.cover.unwrap();
        let viz = l.viz.unwrap();
        assert!(cover.y + cover.height < l.meta.y, "gap after the cover");
        assert!(
            l.meta.y + l.meta.height < viz.y,
            "gap between the metadata and the visualizer"
        );
        assert_eq!(l.progress.y, viz.y + viz.height);
    }

    #[test]
    fn the_block_is_vertically_centred() {
        let l = layout(176, 60);
        let cover = l.cover.unwrap();
        let bottom = l.progress.y + l.progress.height;
        let top_gap = cover.y;
        let bottom_gap = 60 - bottom;
        assert!(
            top_gap.abs_diff(bottom_gap) <= 1,
            "top {top_gap} vs bottom {bottom_gap}"
        );
    }

    #[test]
    fn a_short_pane_still_produces_a_usable_layout() {
        // Everything must stay inside the area even when space runs out.
        for h in 8..=20u16 {
            let l = layout(80, h);
            let bottom = l.progress.y + l.progress.height;
            assert!(bottom <= h, "height {h}: progress bar at {bottom}");
        }
    }

    #[test]
    fn without_a_cover_the_column_still_centres() {
        let l = compute_track_layout(Rect::new(0, 0, 120, 30), (8, 16), 6, false);
        assert!(l.cover.is_none());
        assert!(l.meta.x > 0, "should be indented, not flush left");
        let right_gap = 120 - (l.meta.x + l.meta.width);
        assert!(l.meta.x.abs_diff(right_gap) <= 1);
    }

    #[test]
    fn the_track_panel_inset_is_applied_exactly_once() {
        // cover_rect feeds the graphics placement, so if it disagreed with
        // the renderer by even one cell the artwork would sit on the border.
        let body = Rect::new(0, 2, 112, 24);
        let inner = track_inner(body);
        assert_eq!(inner, Rect::new(1, 3, 110, 22));
        // The layout the renderer uses is computed from that same inner area.
        let l = compute_track_layout(inner, (8, 16), 6, true);
        let cover = l.cover.expect("cover expected");
        assert!(cover.y >= inner.y, "cover must clear the top border");
        assert!(
            cover.x + cover.width <= inner.x + inner.width,
            "cover must clear the right border"
        );
        assert!(
            l.progress.y + l.progress.height <= inner.y + inner.height,
            "progress bar must clear the bottom border"
        );
    }

    #[test]
    fn the_tab_rule_breaks_around_the_active_tab() {
        let r: Vec<char> = tab_rule(20, 5, 10).chars().collect();
        assert_eq!(r.len(), 20);
        // Rule runs up to the corner, gap over the tab, then resumes.
        assert_eq!(r[0], '─');
        assert_eq!(r[4], '┘', "corner just before the tab");
        assert_eq!(&r[5..10], &[' ', ' ', ' ', ' ', ' '], "gap under the tab");
        assert_eq!(r[10], '└', "corner just after the tab");
        assert_eq!(r[19], '─');
    }

    #[test]
    fn the_tab_rule_is_unbroken_when_no_tab_is_active() {
        assert_eq!(tab_rule(8, 0, 0), "─".repeat(8));
    }

    #[test]
    fn the_tab_rule_clamps_to_the_available_width() {
        // An active tab running past the edge must not panic or overrun.
        let r = tab_rule(10, 6, 40);
        assert_eq!(r.chars().count(), 10);
        assert!(r.starts_with("─────"));
    }

    #[test]
    fn columns_are_padded_to_exact_widths() {
        assert_eq!(fit("ab", 5), "ab   ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(rfit("3:58", 6), "  3:58");
        assert_eq!(rfit("12", 2), "12");
        // A wide-character column must not overflow its budget.
        assert_eq!(fit("日本", 4).width(), 4);
    }

    #[test]
    fn window_shows_everything_when_it_fits() {
        assert_eq!(window(0, 5, 10), (0, 5));
        assert_eq!(window(4, 5, 5), (0, 5));
    }

    #[test]
    fn window_centres_the_selection_and_clamps_at_the_ends() {
        // Selection near the top stays at the top.
        assert_eq!(window(0, 100, 10), (0, 10));
        // Middle selection is centred.
        assert_eq!(window(50, 100, 10), (45, 55));
        // Selection at the end pins the window to the bottom.
        assert_eq!(window(99, 100, 10), (90, 100));
    }

    #[test]
    fn window_handles_empty_and_zero_height() {
        assert_eq!(window(0, 0, 10), (0, 0));
        assert_eq!(window(0, 10, 0), (0, 0));
    }

    #[test]
    fn truncate_adds_an_ellipsis_only_when_needed() {
        assert_eq!(elide("abc", 10), "abc");
        assert_eq!(elide("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn truncate_counts_wide_characters() {
        // Each CJK char is two columns wide.
        let s = "日本語テキスト";
        let out = elide(s, 6);
        assert!(out.width() <= 6, "got width {}", out.width());
    }
}
