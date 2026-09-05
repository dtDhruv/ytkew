//! Things drawn on top of a view: the menu, options, help and lyrics.

use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};
use ratatui::Frame;

use super::views::{centered_message, centre, fit, panel, wrap};

/// The overlay: btop's three block-letter entries, or the options pane once
/// Options is chosen.
pub(super) fn draw_menu(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::MenuScreen;
    match app.menu_screen {
        MenuScreen::Main => draw_menu_main(f, area, app),
        MenuScreen::Options => draw_options(f, area, app),
    }
}

pub(super) fn draw_menu_main(f: &mut Frame, area: Rect, app: &App) {
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

/// The options pane. Each setting is two rows -- its name, then its value --
/// with arrows beside the value of the selected row, and that row's
/// description below the list. This is btop's options layout.
pub(super) fn draw_options(f: &mut Frame, area: Rect, app: &App) {
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
                Span::styled(
                    "←",
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(centre(&value, inner_w), Style::default().fg(accent)),
                Span::styled(
                    "→",
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
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

pub(super) fn draw_help(f: &mut Frame, area: Rect, app: &App) {
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
            Span::styled(fit(&e.1, COL - 25), Style::default().fg(accent)),
        ]
    };

    let mut lines: Vec<Line> = Vec::new();
    if two_col {
        for r in 0..rows {
            let mut spans = cell(&entries[r]);
            if let Some(right) = entries.get(r + rows) {
                spans.push(Span::styled(
                    "│",
                    Style::default().fg(app.palette.dark.to_color()),
                ));
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

pub(super) fn draw_lyrics(f: &mut Frame, area: Rect, app: &App) {
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

/// The wordmark banner, centred and shaded down the ramp.
pub(super) fn banner_lines(_app: &App, width: usize) -> Vec<Line<'static>> {
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
