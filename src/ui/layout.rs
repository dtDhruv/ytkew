//! Geometry shared by the renderer and the album-art placement.
//!
//! Both go through the same functions here, so the reserved cells and the
//! image written over them can never disagree.

use crate::app::App;
use crate::config::VisualizerMode;
use crate::ui::View;
use ratatui::layout::Rect;

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
/// Rows of chrome above the content: the tab strip and its rule.
pub const TABBAR_ROWS: u16 = 2;

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

/// Rows of chrome below: the now-playing bar plus the hint line. The track
/// view is itself the player, so it does not repeat the bar.
pub fn footer_rows(app: &App) -> u16 {
    if app.view == View::Track || app.queue.current().is_none() {
        1
    } else {
        2
    }
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
}
