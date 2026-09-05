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

pub fn track_layout(area: Rect, app: &App, fill: bool) -> TrackLayout {
    let viz_h = if matches!(app.cfg.visualizer_mode, VisualizerMode::Off) {
        0
    } else {
        app.cfg.visualizer_height
    };
    let draws_cover = app.cover_visible && app.cfg.cover_mode.draws_anything();
    compute_track_layout_with(area, app.cell_px, viz_h, draws_cover, fill)
}

/// The geometry, free of `App` so it can be tested directly.
pub fn compute_track_layout(
    area: Rect,
    cell_px: (u16, u16),
    visualizer_height: u16,
    draws_cover: bool,
) -> TrackLayout {
    compute_track_layout_with(area, cell_px, visualizer_height, draws_cover, false)
}

/// `fill` widens the text column to the whole pane instead of tying it to the
/// cover. Used by the split view, where the pane belongs to the player alone,
/// so a title has no reason to truncate at the cover's edge.
pub fn compute_track_layout_with(
    area: Rect,
    cell_px: (u16, u16),
    visualizer_height: u16,
    draws_cover: bool,
    fill: bool,
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
    let col_w = if fill {
        // One column of breathing room either side of the border.
        area.width.saturating_sub(2).max(1)
    } else {
        cover_w
            .max(if cover_w > 0 {
                MIN_COL_WIDTH
            } else {
                (area.width * 2 / 3).max(24)
            })
            .min(area.width)
    };
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

/// Below this the two panes would both be cramped, so the track view stays a
/// single centred column however the side pane is configured.
pub const SPLIT_MIN_WIDTH: u16 = 88;
/// The narrowest side pane worth drawing: titles below this just elide away.
const SIDE_MIN_WIDTH: u16 = 30;
/// A column between the panels. Two borders meeting flush reads as one thick
/// rule rather than two panels.
const PANE_GAP: u16 = 1;
/// Past this the player column is mostly margin and the list wants the room.
const PLAYER_PANE_MAX: u16 = 56;

/// Split the body into the now-playing panel and an optional side panel.
///
/// A wide terminal leaves the centred column floating in dead space, which is
/// what most terminal players fill with a list -- cmus and spotify-tui both
/// put the browser beside the player rather than under it.
pub fn track_panes(body: Rect, app: &App) -> (Rect, Option<Rect>) {
    compute_track_panes(body, app.cell_px, app.cfg.side_pane.shows_anything())
}

/// The pane geometry, free of `App` so it can be tested directly.
pub fn compute_track_panes(
    body: Rect,
    cell_px: (u16, u16),
    side_pane: bool,
) -> (Rect, Option<Rect>) {
    if !side_pane || body.width < SPLIT_MIN_WIDTH {
        return (body, None);
    }
    // Wide enough for the largest cover it can hold -- the row cap times the
    // cell aspect -- and then a share of what is left, since the title,
    // progress bar and spectrum all widen with the column even though the
    // cover cannot. Capped, because past a point it is the list that wants
    // the room.
    let (cw, ch) = cell_px;
    let ratio = (ch as f32 / cw.max(1) as f32).max(1.0);
    let widest_cover = (COVER_MAX_ROWS as f32 * ratio).round() as u16;
    let floor = widest_cover.saturating_add(6).max(MIN_COL_WIDTH + 4);
    // A very tall cell can push the floor past the cap; the cover wins, since
    // the alternative is clipping it.
    let cap = PLAYER_PANE_MAX.max(floor);
    let left = (body.width * 2 / 5)
        .clamp(floor, cap)
        .min(body.width / 2)
        .min(body.width - SIDE_MIN_WIDTH - PANE_GAP);
    (
        Rect::new(body.x, body.y, left, body.height),
        Some(Rect::new(
            body.x + left + PANE_GAP,
            body.y,
            body.width - left - PANE_GAP,
            body.height,
        )),
    )
}

/// Where the cover goes, for the sixel/kitty painter.
pub fn cover_rect(area: Rect, app: &App) -> Option<Rect> {
    let (player, side) = track_panes(area, app);
    track_layout(track_inner(player), app, side.is_some()).cover
}

/// The region between the tab strip and the footer.
///
/// Takes the app rather than assuming a footer height: the track view has a
/// one-row footer and every other view has two, and the layout centres its
/// content vertically, so guessing wrong shifts the cover by a row -- enough
/// for the reserved cells and the image drawn over them to disagree.
pub fn body_rect(area: Rect, app: &App) -> Rect {
    Rect::new(
        area.x,
        area.y + TABBAR_ROWS,
        area.width,
        area.height.saturating_sub(TABBAR_ROWS + footer_rows(app)),
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

    fn panes(w: u16, h: u16, on: bool) -> (Rect, Option<Rect>) {
        compute_track_panes(Rect::new(0, 2, w, h), (8, 16), on)
    }

    #[test]
    fn a_narrow_terminal_stays_one_column() {
        for w in 40..SPLIT_MIN_WIDTH {
            let (player, side) = panes(w, 30, true);
            assert!(side.is_none(), "width {w} should not split");
            assert_eq!(player.width, w, "the player takes the whole body");
        }
    }

    #[test]
    fn the_panes_never_overlap_and_leave_a_gap() {
        for w in SPLIT_MIN_WIDTH..400 {
            let (player, side) = panes(w, 30, true);
            let side = side.expect("should split");
            assert_eq!(
                side.x,
                player.x + player.width + PANE_GAP,
                "width {w}: panes must not touch"
            );
            assert_eq!(
                player.width + PANE_GAP + side.width,
                w,
                "width {w}: panes must fill the body exactly"
            );
            assert!(
                side.width >= SIDE_MIN_WIDTH,
                "width {w}: side pane {} too narrow",
                side.width
            );
            // The player column must still fit a full-size cover.
            let inner = track_inner(player);
            let l = compute_track_layout(inner, (8, 16), 6, true);
            let cover = l.cover.expect("cover expected");
            assert!(
                cover.x + cover.width <= inner.x + inner.width,
                "width {w}: cover overflows the player pane"
            );
        }
    }

    #[test]
    fn turning_the_side_pane_off_gives_the_whole_body_back() {
        let (player, side) = panes(200, 40, false);
        assert!(side.is_none());
        assert_eq!(player.width, 200);
    }

    #[test]
    fn the_player_pane_does_not_hoard_width_it_cannot_use() {
        // It widens with the terminal, because the title, progress bar and
        // spectrum all use the extra room -- but only up to a point, or the
        // list loses the space the split was meant to reclaim.
        let (player, side) = panes(300, 40, true);
        assert!(
            player.width <= PLAYER_PANE_MAX,
            "player pane {} past the cap",
            player.width
        );
        assert!(
            side.unwrap().width > player.width,
            "past the cap the list should get the majority"
        );
    }

    #[test]
    fn the_player_pane_widens_with_the_terminal_up_to_the_cap() {
        let narrow = panes(SPLIT_MIN_WIDTH, 40, true).0.width;
        let mid = panes(130, 40, true).0.width;
        assert!(mid > narrow, "{mid} should exceed {narrow}");
        assert!(mid <= PLAYER_PANE_MAX);
    }

    #[test]
    fn filling_widens_the_text_column_without_moving_the_cover() {
        // The point of `fill`: in a pane that belongs to the player alone, a
        // title has no reason to truncate at the cover's edge.
        let area = Rect::new(1, 3, 50, 24);
        let tied = compute_track_layout_with(area, (8, 16), 6, true, false);
        let filled = compute_track_layout_with(area, (8, 16), 6, true, true);
        assert!(
            filled.meta.width > tied.meta.width,
            "filled column {} should beat {}",
            filled.meta.width,
            tied.meta.width
        );
        assert_eq!(
            filled.cover.unwrap().width,
            tied.cover.unwrap().width,
            "the cover stays square; only the text column grows"
        );
        assert!(
            filled.meta.x + filled.meta.width <= area.x + area.width,
            "the column must stay inside the pane"
        );
    }

    #[test]
    fn a_taller_cell_gets_a_wider_player_pane() {
        // A square cover needs more columns when cells are tall and narrow,
        // and the pane has to keep up or the art gets clipped.
        let square = compute_track_panes(Rect::new(0, 2, 200, 40), (8, 16), true).0;
        let tall = compute_track_panes(Rect::new(0, 2, 200, 40), (6, 18), true).0;
        assert!(
            tall.width > square.width,
            "tall cells {} should widen the pane past {}",
            tall.width,
            square.width
        );
    }

    #[test]
    fn the_footer_height_changes_where_the_cover_sits() {
        // The regression this guards: assuming a two-row footer on the track
        // view moved the painted image one row off the reserved cells, and
        // the gap showed the previous view's text.
        let frame = Rect::new(0, 0, 100, 34);
        let one = Rect::new(
            frame.x,
            frame.y + TABBAR_ROWS,
            frame.width,
            frame.height - TABBAR_ROWS - 1,
        );
        let two = Rect::new(
            frame.x,
            frame.y + TABBAR_ROWS,
            frame.width,
            frame.height - TABBAR_ROWS - 2,
        );
        let a = compute_track_layout(track_inner(one), (8, 16), 6, true);
        let b = compute_track_layout(track_inner(two), (8, 16), 6, true);
        assert_ne!(
            a.cover.unwrap().y,
            b.cover.unwrap().y,
            "a one-row difference does move the cover"
        );
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
