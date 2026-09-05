//! Persistent chrome: the tab strip, the now-playing bar and the hint line.

use crate::app::App;
use crate::model::fmt_duration;
use crate::ui::View;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::track::effective_duration;
use super::views::fit;

/// Browser-style tab strip: a rule under the whole width that breaks around
/// the active tab, so the tab reads as joined to the content below it.
pub(super) fn draw_tabbar(f: &mut Frame, area: Rect, app: &mut App) {
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

    // Transport state, right-aligned.
    let mut right = status_spans(app, accent, dim, faint);
    let left_w = col as usize;
    let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
    if left_w + right_w < area.width as usize {
        spans.push(Span::raw(
            " ".repeat(area.width as usize - left_w - right_w),
        ));
        spans.append(&mut right);
    }
    Paragraph::new(Line::from(spans))
        .render(Rect::new(area.x, area.y, area.width, 1), f.buffer_mut());

    app.hits.tabs = hit_tabs;

    // The rule, with a gap where the active tab sits.
    let (a, b) = active.unwrap_or((0, 0));
    let rule = tab_rule(area.width as usize, a as usize, b as usize);
    Paragraph::new(Line::from(Span::styled(rule, Style::default().fg(faint))))
        .render(Rect::new(area.x, area.y + 1, area.width, 1), f.buffer_mut());
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
pub(super) fn draw_player_bar(f: &mut Frame, area: Rect, app: &mut App) {
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

pub(super) fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
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
            spans.push(Span::styled(
                " · ",
                Style::default().fg(app.palette.dark.to_color()),
            ));
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

/// The rule under the tab strip, broken around the active tab so the tab
/// reads as joined to the content below it -- the same trick a browser's tab
/// bar uses.
pub(super) fn tab_rule(width: usize, active_start: usize, active_end: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
