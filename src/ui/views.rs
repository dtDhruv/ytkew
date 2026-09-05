//! The frame: chrome, the active pane, and any overlay on top.

use crate::app::App;
use crate::ui::chrome::{draw_footer, draw_player_bar, draw_tabbar};
use crate::ui::layout::{body_rect, footer_rows, TABBAR_ROWS};
use crate::ui::lists::{draw_library, draw_queue, draw_search};
use crate::ui::overlay::{draw_help, draw_lyrics, draw_menu};
use crate::ui::track::draw_track;
use crate::ui::widgets::elide;
use crate::ui::View;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

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
    // Shared with the graphics painter so the reserved cells and the image
    // written over them always land on the same rows.
    let body = body_rect(area, app);
    let footer = Rect::new(area.x, area.y + area.height - foot_h, area.width, foot_h);

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
        draw_footer(f, Rect::new(footer.x, footer.y + 1, footer.width, 1), app);
    } else {
        draw_footer(f, footer, app);
    }
}

/// Centre text in exactly `w` columns, padded on both sides so a highlighted
/// row spans the full width and the right-hand arrow sits at the edge.
pub(super) fn centre(s: &str, w: usize) -> String {
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
pub(super) fn wrap(text: &str, w: usize, max_lines: usize) -> Vec<String> {
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

/// Mark a region as skipped so the backend emits nothing for it.
pub(super) fn reserve_cells(f: &mut Frame, area: Rect) {
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
pub(super) fn panel(
    f: &mut Frame,
    area: Rect,
    title: &str,
    count: Option<(usize, usize)>,
    app: &App,
) -> Rect {
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
pub(super) fn scrollbar(f: &mut Frame, area: Rect, position: usize, total: usize, app: &App) {
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
pub(super) fn fit(s: &str, w: usize) -> String {
    let t = elide(s, w as u16);
    let used = t.width();
    let mut out = t;
    for _ in used..w {
        out.push(' ');
    }
    out
}

/// Right-align text within `w` columns.
pub(super) fn rfit(s: &str, w: usize) -> String {
    let t = elide(s, w as u16);
    let used = t.width();
    let mut out = String::new();
    for _ in used..w {
        out.push(' ');
    }
    out.push_str(&t);
    out
}

pub(super) fn centered_message(f: &mut Frame, area: Rect, msg: &str) {
    Paragraph::new(msg)
        .alignment(Alignment::Center)
        .render(centered_row(area), f.buffer_mut());
}

pub(super) fn centered_row(area: Rect) -> Rect {
    Rect::new(area.x, area.y + area.height / 2, area.width, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

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
