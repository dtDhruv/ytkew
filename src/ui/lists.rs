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

/// Narrowest column worth drawing: below this, labels are all ellipsis.
const LIB_COL_MIN_WIDTH: u16 = 20;

/// How many columns the miller view gets, or 0 to fall back to the stacked
/// tree. Two is the minimum that says anything the tree does not.
fn library_column_count(inner_width: u16) -> usize {
    let fits = (inner_width / LIB_COL_MIN_WIDTH) as usize;
    if fits < 2 {
        0
    } else {
        fits.min(4)
    }
}

/// The library as side-by-side columns, one per level, in the manner of a
/// file manager: the chain you walked to get here stays on screen instead of
/// being implied by indentation.
///
/// Falls back to the stacked tree when the pane is too narrow for two
/// columns, which is what happens in the track view's side pane and on a
/// small terminal.
pub(super) fn draw_library(f: &mut Frame, area: Rect, app: &mut App) {
    let want = app.cfg.library_layout == crate::config::LibraryLayout::Columns;
    let cols = if want {
        library_column_count(area.width.saturating_sub(2))
    } else {
        0
    };
    app.library_columns_open = cols >= 2 && !app.library_rows.is_empty();
    if app.library_columns_open {
        draw_library_columns(f, area, app, cols);
    } else {
        draw_library_tree(f, area, app);
    }
}

fn draw_library_columns(f: &mut Frame, area: Rect, app: &mut App, max_cols: usize) {
    let columns = app.library_columns();
    if columns.is_empty() {
        app.library_columns_open = false;
        return draw_library_tree(f, area, app);
    }
    // Position within the level being read, which is the useful number here:
    // the depth is already obvious from how many columns are on screen.
    let count = columns
        .iter()
        .rfind(|c| c.selected.is_some())
        .map(|c| (c.selected.unwrap_or(0), c.rows.len()));
    let inner = panel(f, area, "library", count, app);

    // Show the deepest columns: the cursor and where it can go next matter
    // more than the root you already left.
    let first = columns.len().saturating_sub(max_cols);
    let shown = &columns[first..];
    let n = shown.len() as u16;
    let seps = n.saturating_sub(1);
    // The rightmost column carries the longest labels -- track titles, where
    // the parent levels are short category and playlist names -- so give it
    // twice the share rather than dividing evenly and truncating it.
    let shares = (n + 1) as u32;
    let unit = (inner.width.saturating_sub(seps)) as u32 / shares;
    if unit == 0 {
        app.library_columns_open = false;
        return draw_library_tree(f, area, app);
    }

    let accent = app.palette.accent().to_color();
    let dim = app.palette.secondary().to_color();
    let faint = app.palette.dark.to_color();
    // The focused column is the last one the cursor runs through; anything to
    // its right is a preview of what stepping in would show.
    let focused = shown.iter().rposition(|c| c.selected.is_some());

    let mut hits = Vec::new();
    let mut x = inner.x;
    for (ci, col) in shown.iter().enumerate() {
        // The last column takes the remainder, so the panel fills exactly.
        let w = if ci + 1 == shown.len() {
            (inner.x + inner.width).saturating_sub(x)
        } else {
            unit as u16
        };
        let rect = Rect::new(x, inner.y, w, inner.height);
        let is_focused = focused == Some(ci);
        let sel = col.selected.unwrap_or(0);
        let (start, end) = window(sel, col.rows.len(), rect.height as usize);
        let bar_w: u16 = if col.rows.len() > rect.height as usize {
            1
        } else {
            0
        };
        let text_w = rect.width.saturating_sub(bar_w + 2) as usize;

        let lines: Vec<Line> = (start..end)
            .map(|i| {
                let Some(node) = app.node_at(&col.rows[i]) else {
                    return Line::from("");
                };
                let is_song = node.kind.is_song();
                // Whether stepping right leads anywhere, without needing to.
                let arrow = if is_song {
                    " "
                } else if node.loading {
                    "\u{22ef}"
                } else {
                    "\u{25b8}"
                };
                let base = if is_song {
                    Style::default().fg(faint)
                } else {
                    Style::default().fg(dim)
                };
                let style = match (col.selected == Some(i), is_focused) {
                    // Only the focused column gets a solid cursor; the trail
                    // behind it is marked, not competing for attention.
                    (true, true) => Style::default()
                        .fg(accent)
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                    (true, false) => base.fg(accent).add_modifier(Modifier::BOLD),
                    _ => base,
                };
                Line::from(vec![
                    Span::styled(fit(&format!(" {}", node.kind.label()), text_w), style),
                    Span::styled(format!(" {arrow}"), Style::default().fg(faint)),
                ])
            })
            .collect();
        Paragraph::new(lines).render(rect, f.buffer_mut());
        if bar_w == 1 {
            scrollbar(f, rect, sel, col.rows.len(), app);
        }
        hits.push((rect, start, first + ci));

        x += w;
        if ci + 1 < shown.len() {
            // A rule rather than a gap: it reads as one browser split into
            // levels rather than several unrelated lists.
            let sep: Vec<Line> = (0..inner.height)
                .map(|_| Line::from(Span::styled("\u{2502}", Style::default().fg(faint))))
                .collect();
            Paragraph::new(sep).render(Rect::new(x, inner.y, 1, inner.height), f.buffer_mut());
            x += 1;
        }
    }
    app.hits.list = None;
    app.hits.lib_columns = hits;
}

fn draw_library_tree(f: &mut Frame, area: Rect, app: &mut App) {
    app.hits.lib_columns.clear();
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

    // The filter strip: YouTube Music searches one kind at a time, so which
    // kind is showing has to be on screen, not remembered.
    let mut spans = vec![Span::raw(" ")];
    for (i, filter) in crate::app::SEARCH_FILTERS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(faint)));
        }
        let active = *filter == app.search_filter;
        spans.push(Span::styled(
            filter.name(),
            if active {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(faint)
            },
        ));
    }
    spans.push(Span::styled("   ← → to switch", Style::default().fg(faint)));
    Paragraph::new(Line::from(spans))
        .render(Rect::new(area.x, area.y + 2, area.width, 1), f.buffer_mut());

    let list = Rect::new(
        area.x,
        area.y + 3,
        area.width,
        area.height.saturating_sub(3),
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
    // The trailing column holds a duration or a kind, so it needs more than
    // the six columns a duration alone would.
    let tail_w = 9;
    let fixed = 2 + idx_w + 2 + tail_w + bar_w;
    let flex = (list.width as usize).saturating_sub(fixed);
    let sub_w = (flex * 2 / 5).min(flex);
    let label_w = flex - sub_w;

    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let hit = &app.search_results[i];
            let is_sel = i == app.search_sel;
            let row = format!(
                "  {} {} {}{}",
                rfit(&(i + 1).to_string(), idx_w),
                fit(&hit.sublabel(), sub_w),
                fit(&hit.label(), label_w),
                rfit(&hit.trailing(), tail_w),
            );
            let base = if is_sel {
                Style::default().fg(dim)
            } else if hit.track().is_none() {
                // Containers read as things to open, like the library's.
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
