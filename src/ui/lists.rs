//! The scrollable panes: queue, library tree and search results.

use crate::app::App;
use crate::ui::widgets::elide;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

use super::views::{centered_message, fit, panel, rfit, scrollbar, window};

/// The queue from the current track onward, for the track view's side pane.
///
/// Deliberately not the full queue pane: what has already played is dead
/// weight beside a now-playing column, and a narrow pane cannot afford the
/// index and artist columns the queue view uses.
pub(super) fn draw_up_next(f: &mut Frame, area: Rect, app: &mut App) {
    let start = app.queue.current_index().map_or(0, |i| i + 1);
    let total = app.queue.len();
    let upcoming = total.saturating_sub(start);
    let inner = panel(f, area, "up next", None, app);
    if upcoming == 0 {
        let msg = if total == 0 {
            "nothing queued"
        } else {
            "end of the queue"
        };
        return centered_message(f, inner, msg);
    }

    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();
    // The selection is the queue's, so moving here and switching to the queue
    // pane lands on the same track.
    let sel = app.queue_sel.max(start).min(total - 1);
    let (win_start, win_end) = window(sel - start, upcoming, inner.height as usize);
    let (win_start, win_end) = (start + win_start, start + win_end);

    let bar_w = if upcoming > inner.height as usize {
        1
    } else {
        0
    };
    // Duration plus the two spaces before it; the rest is the title, with an
    // artist column only once there is room for both to stay readable.
    let flex = (inner.width as usize).saturating_sub(2 + 6 + bar_w);
    let artist_w = if flex >= 44 { flex * 2 / 5 } else { 0 };
    let title_w = flex - artist_w;

    let lines: Vec<Line> = (win_start..win_end)
        .map(|i| {
            let t = &app.queue.tracks()[i];
            let is_sel = i == sel;
            let row = if artist_w > 0 {
                format!(
                    "  {} {}{}",
                    fit(&t.artist, artist_w.saturating_sub(1)),
                    fit(&t.title, title_w),
                    rfit(&t.duration_text, 6),
                )
            } else {
                format!("  {}{}", fit(&t.title, title_w), rfit(&t.duration_text, 6),)
            };
            let base = Style::default().fg(if is_sel { dim } else { faint });
            let style = if is_sel {
                base.add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            Line::from(Span::styled(elide(&row, inner.width - bar_w as u16), style))
        })
        .collect();
    Paragraph::new(lines).render(inner, f.buffer_mut());

    if bar_w == 1 {
        scrollbar(f, inner, sel - start, upcoming, app);
    }
}

pub(super) fn draw_queue(f: &mut Frame, area: Rect, app: &mut App) {
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

pub(super) fn draw_library(f: &mut Frame, area: Rect, app: &mut App) {
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

pub(super) fn draw_search(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.search_results.len();
    let area = panel(
        f,
        area,
        "search",
        (n > 0).then_some((app.search_sel, n)),
        app,
    );
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
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ]);
    Paragraph::new(prompt).render(Rect::new(area.x, area.y, area.width, 1), f.buffer_mut());
    // A rule under the input separates query from results.
    Paragraph::new(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(faint),
    )))
    .render(Rect::new(area.x, area.y + 1, area.width, 1), f.buffer_mut());

    let list = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(2),
    );

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
