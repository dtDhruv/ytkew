//! Drawing primitives for the track view, laid out to match kew.

use crate::config::VisualizerMode;
use crate::model::{fmt_duration, Track};
use crate::palette::Palette;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;
#[cfg(test)]
use unicode_width::UnicodeWidthStr;

/// Eighth-block ramp, low to high. kew uses exactly this set.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// kew's braille ramp -- four levels per cell instead of eight.
const BRAILLE: [char; 4] = ['⣀', '⣤', '⣶', '⣿'];

/// Draw a cover as upper-half-blocks: fg is the top pixel, bg the bottom.
pub fn draw_cover(f: &mut Frame, area: Rect, cover: &crate::cover::Cover) {
    let buf = f.buffer_mut();
    for (r, row) in cover.cells.iter().enumerate() {
        let y = area.y + r as u16;
        if y >= area.y + area.height {
            break;
        }
        for (c, (top, bot)) in row.iter().enumerate() {
            let x = area.x + c as u16;
            if x >= area.x + area.width {
                break;
            }
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char('▀')
                    .set_fg(top.to_color())
                    .set_bg(bot.to_color());
            }
        }
    }
}

/// While a cover is still downloading, kew shows a flat chroma frame rather
/// than empty space, which keeps the layout from jumping.
pub fn draw_cover_placeholder(f: &mut Frame, area: Rect, palette: &Palette) {
    let buf = f.buffer_mut();
    let c = palette.dark.to_color();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char('▀').set_fg(c).set_bg(c);
            }
        }
    }
}

/// Title, artist, album -- in kew's order, with a clear weight hierarchy so
/// the eye finds the track name first.
pub fn draw_metadata(f: &mut Frame, area: Rect, track: &Track, palette: &Palette) {
    let accent = palette.accent().to_color();
    let secondary = palette.secondary().to_color();
    let dim = palette.dark.to_color();
    let width = area.width;

    let rows = vec![
        Line::from(Span::styled(
            elide(&track.title, width),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            elide(&track.artist, width),
            Style::default().fg(secondary),
        )),
        Line::from(Span::styled(
            elide(&track.album.clone().unwrap_or_default(), width),
            // The album is context, not identity, so it recedes.
            Style::default().fg(dim),
        )),
    ];
    Paragraph::new(rows).render(area, f.buffer_mut());
}

/// Truncate to a display width, respecting wide characters.
pub fn elide(s: &str, width: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let max = width as usize;
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.to_string().width();
        if w + cw > max.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

/// Spectrum bars. `values` are 0.0..=1.0, one per bar.
pub fn draw_visualizer(
    f: &mut Frame,
    area: Rect,
    values: &[f32],
    palette: &Palette,
    mode: VisualizerMode,
    bar_width: u16,
) {
    if area.height == 0 || matches!(mode, VisualizerMode::Off) {
        return;
    }
    let bw = bar_width.max(1);
    let gradient = palette.gradient(area.height as usize);
    let (levels, ramp): (usize, &[char]) = match mode {
        VisualizerMode::Braille => (BRAILLE.len(), &BRAILLE),
        _ => (BLOCKS.len(), &BLOCKS),
    };

    let buf = f.buffer_mut();
    for (i, v) in values.iter().enumerate() {
        let x0 = area.x + i as u16 * bw;
        if x0 >= area.x + area.width {
            break;
        }
        // Total sub-cell units this bar fills, bottom-up.
        let units = (v.clamp(0.0, 1.0) * (area.height as f32) * levels as f32).round() as i32;

        for row in 0..area.height {
            // row 0 is the top of the region, so invert for a bottom-up bar.
            let from_bottom = (area.height - 1 - row) as i32;
            let remaining = units - from_bottom * levels as i32;
            if remaining <= 0 {
                continue;
            }
            let ch = if remaining >= levels as i32 {
                ramp[levels - 1]
            } else {
                ramp[(remaining - 1).clamp(0, levels as i32 - 1) as usize]
            };
            // Gradient indexed so the accent sits at the top of the bar.
            let color = gradient
                .get(from_bottom as usize)
                .copied()
                .map(|c| c.to_color())
                .unwrap_or(Color::Cyan);
            for dx in 0..bw {
                let x = x0 + dx;
                if x >= area.x + area.width {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, area.y + row)) {
                    cell.set_char(ch).set_fg(color);
                }
            }
        }
    }
}

/// Progress bar with the elapsed and total times flanking it and a marker at
/// the playhead.
///
/// kew distinguishes elapsed from remaining by colour alone, which reads
/// poorly at a glance; a marker gives the eye something to land on, and the
/// times belong next to the thing they describe rather than on a separate row.
/// Returns the x position and width of the bar's track, so a click on it can
/// be turned into a seek. None when there was no room to draw a bar.
pub fn draw_progress(
    f: &mut Frame,
    area: Rect,
    elapsed: f64,
    duration: f64,
    palette: &Palette,
) -> Option<(u16, u16)> {
    if area.width == 0 {
        return None;
    }
    let left_label = fmt_duration(elapsed);
    let right_label = if duration > 0.0 {
        fmt_duration(duration)
    } else {
        "--:--".into()
    };
    // One space either side of the bar.
    let labels = left_label.len() + right_label.len() + 2;
    let bar_w = (area.width as usize).saturating_sub(labels);
    if bar_w < 4 {
        // Too narrow for a bar; the times alone are more useful.
        Paragraph::new(Line::from(Span::styled(
            format!("{left_label} / {right_label}"),
            Style::default().fg(palette.secondary().to_color()),
        )))
        .render(area, f.buffer_mut());
        return None;
    }

    let frac = if duration > 0.0 {
        (elapsed / duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // The marker occupies one cell, so the filled run is at most bar_w - 1.
    let filled = ((frac * bar_w as f64).round() as usize).min(bar_w.saturating_sub(1));
    let remaining = bar_w - filled - 1;

    Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{left_label} "),
            Style::default().fg(palette.secondary().to_color()),
        ),
        Span::styled(
            "━".repeat(filled),
            Style::default().fg(palette.accent().to_color()),
        ),
        Span::styled(
            "●",
            Style::default()
                .fg(palette.accent().to_color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "━".repeat(remaining),
            Style::default().fg(palette.dark.to_color()),
        ),
        Span::styled(
            format!(" {right_label}"),
            Style::default().fg(palette.secondary().to_color()),
        ),
    ]))
    .render(area, f.buffer_mut());

    Some((area.x + left_label.len() as u16 + 1, bar_w as u16))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render<F: FnOnce(&mut Frame)>(w: u16, h: u16, f: F) -> ratatui::buffer::Buffer {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|frame| f(frame)).unwrap();
        t.backend().buffer().clone()
    }

    fn cell_char(buf: &ratatui::buffer::Buffer, x: u16, y: u16) -> char {
        buf.cell(Position::new(x, y))
            .unwrap()
            .symbol()
            .chars()
            .next()
            .unwrap()
    }

    #[test]
    fn full_bar_fills_the_column_bottom_up() {
        let buf = render(4, 4, |f| {
            let area = Rect::new(0, 0, 4, 4);
            draw_visualizer(f, area, &[1.0], &Palette::default(), VisualizerMode::Bars, 1);
        });
        for y in 0..4 {
            assert_eq!(cell_char(&buf, 0, y), '█', "row {y} should be full");
        }
    }

    #[test]
    fn silence_draws_nothing() {
        let buf = render(4, 4, |f| {
            let area = Rect::new(0, 0, 4, 4);
            draw_visualizer(f, area, &[0.0], &Palette::default(), VisualizerMode::Bars, 1);
        });
        for y in 0..4 {
            assert_eq!(cell_char(&buf, 0, y), ' ');
        }
    }

    #[test]
    fn half_height_fills_only_the_bottom_half() {
        let buf = render(2, 4, |f| {
            let area = Rect::new(0, 0, 2, 4);
            draw_visualizer(f, area, &[0.5], &Palette::default(), VisualizerMode::Bars, 1);
        });
        // Bottom two rows solid, top two empty.
        assert_eq!(cell_char(&buf, 0, 3), '█');
        assert_eq!(cell_char(&buf, 0, 2), '█');
        assert_eq!(cell_char(&buf, 0, 0), ' ');
    }

    #[test]
    fn bar_width_widens_each_bar() {
        let buf = render(6, 2, |f| {
            let area = Rect::new(0, 0, 6, 2);
            draw_visualizer(f, area, &[1.0, 1.0], &Palette::default(), VisualizerMode::Bars, 2);
        });
        for x in 0..4 {
            assert_eq!(cell_char(&buf, x, 1), '█', "col {x} should be filled");
        }
        assert_eq!(cell_char(&buf, 4, 1), ' ');
    }

    #[test]
    fn progress_shows_elapsed_and_total_around_the_bar() {
        let buf = render(40, 1, |f| {
            draw_progress(f, Rect::new(0, 0, 40, 1), 83.0, 296.0, &Palette::default());
        });
        let row: String = (0..40).map(|x| cell_char(&buf, x, 0)).collect();
        assert!(row.starts_with("1:23 "), "elapsed should lead: {row:?}");
        assert!(row.ends_with(" 4:56"), "total should trail: {row:?}");
    }

    #[test]
    fn the_playhead_marker_tracks_position() {
        let at = |elapsed: f64| {
            let buf = render(40, 1, |f| {
                draw_progress(f, Rect::new(0, 0, 40, 1), elapsed, 100.0, &Palette::default());
            });
            (0..40)
                .position(|x| cell_char(&buf, x, 0) == '●')
                .expect("marker should be drawn")
        };
        let (start, mid, end) = (at(0.0), at(50.0), at(100.0));
        assert!(start < mid && mid < end, "{start} {mid} {end}");
        // At the start the marker sits right after the elapsed label.
        assert_eq!(start, 5, "expected the marker at the bar's left edge");
    }

    #[test]
    fn zero_duration_progress_does_not_divide_by_zero() {
        let buf = render(40, 1, |f| {
            draw_progress(f, Rect::new(0, 0, 40, 1), 0.0, 0.0, &Palette::default());
        });
        let row: String = (0..40).map(|x| cell_char(&buf, x, 0)).collect();
        assert!(row.contains("--:--"), "unknown duration should show as --:--");
    }

    #[test]
    fn a_narrow_progress_area_falls_back_to_plain_times() {
        let buf = render(12, 1, |f| {
            draw_progress(f, Rect::new(0, 0, 12, 1), 83.0, 296.0, &Palette::default());
        });
        let row: String = (0..12).map(|x| cell_char(&buf, x, 0)).collect();
        assert!(row.starts_with("1:23 / 4:56"), "got {row:?}");
        assert!(!row.contains('●'), "no room for a bar, so no marker");
    }

    #[test]
    fn elide_respects_wide_characters() {
        assert_eq!(elide("abc", 10), "abc");
        assert_eq!(elide("abcdefghij", 5), "abcd…");
        assert!(elide("日本語テキスト", 6).width() <= 6);
    }

    #[test]
    fn braille_mode_uses_braille_glyphs() {
        let buf = render(2, 2, |f| {
            let area = Rect::new(0, 0, 2, 2);
            draw_visualizer(f, area, &[1.0], &Palette::default(), VisualizerMode::Braille, 1);
        });
        assert_eq!(cell_char(&buf, 0, 1), '⣿');
    }

    #[test]
    fn off_mode_draws_nothing() {
        let buf = render(2, 2, |f| {
            let area = Rect::new(0, 0, 2, 2);
            draw_visualizer(f, area, &[1.0], &Palette::default(), VisualizerMode::Off, 1);
        });
        assert_eq!(cell_char(&buf, 0, 1), ' ');
    }
}
