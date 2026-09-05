//! Album-art rendering state: which protocol is in use, painting it over
//! the reserved cells, and removing it again.

use crate::config::Config;
use crate::palette::Palette;

use super::*;

/// How the cover is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graphics {
    /// Kitty graphics: sized in cells, so no cell pixel size is involved.
    /// Preferred wherever it is available.
    Kitty,
    /// Sixel: needs an accurate cell pixel size.
    Sixel,
    /// Unicode half-blocks inside ratatui's own buffer.
    None,
}

impl Graphics {
    pub(crate) fn resolve(cfg: &Config, cell_source: crate::art::terminal::CellSource) -> Self {
        // Kitty first: it is the only protocol that sidesteps cell-size
        // detection entirely, which is what every sizing bug here came from.
        if cfg
            .cover_mode
            .uses_kitty(crate::art::kitty::terminal_supports_kitty())
        {
            return Graphics::Kitty;
        }
        if cfg
            .cover_mode
            .uses_sixel(crate::art::terminal::sixel_recommended(cell_source))
        {
            return Graphics::Sixel;
        }
        Graphics::None
    }
}

/// The palette a theme name resolves to. `cover` has no fixed colours -- it
/// starts from the accent and is replaced once artwork loads.
pub(crate) fn theme_palette(name: &str, cfg: &Config, accent: u8) -> Palette {
    if name.eq_ignore_ascii_case("custom") {
        if let Some(p) = crate::theme::from_hex(&cfg.theme_colors) {
            return p;
        }
    }
    match crate::theme::find(name) {
        Some(t) => t.palette(),
        None => Palette::from_ansi(accent),
    }
}

impl App {
    /// Recompute the palette for the current theme. Under `cover` this waits
    /// for artwork; the accent stands in until then.
    pub(crate) fn apply_theme(&mut self) {
        if self.theme.eq_ignore_ascii_case(crate::theme::COVER) {
            // Fall back to the cover we already have, if any.
            self.palette = match &self.cover {
                Some(c) => c.palette,
                None => Palette::from_ansi(self.cfg.accent_color),
            };
        } else {
            self.palette = theme_palette(&self.theme, &self.cfg, self.cfg.accent_color);
        }
        self.clear_cover_art();
    }

    /// True when colours should follow the artwork.
    pub(crate) fn theme_follows_cover(&self) -> bool {
        self.theme.eq_ignore_ascii_case(crate::theme::COVER)
    }

    /// Adjust the assumed cell height by `delta` px, deriving the width from
    /// the current aspect so the art stays square.
    ///
    /// This exists because no automatic measurement is reliable here: zellij
    /// has an open regression (zellij-org/zellij#3372) that renders sixel at
    /// double height, and nothing the terminal reports reflects that. Letting
    /// the user resize while watching is the only approach that converges.
    pub(crate) fn nudge_cover(&mut self, delta: i32) {
        let (w, h) = self.cell_px;
        let ratio = w as f32 / h.max(1) as f32;
        let new_h = (h as i32 + delta).clamp(6, 80) as u16;
        let new_w = ((new_h as f32 * ratio).round() as u16).max(3);
        self.cell_px = (new_w, new_h);
        // A hand-tuned value is pinned, and so is trusted and persisted.
        self.cell_source = crate::art::terminal::CellSource::Config;
        self.cfg.cell_px = [new_w, new_h];
        self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
        self.clear_cover_art();
        self.notify(format!(
            "cover cell {new_w}x{new_h} — [ smaller, ] bigger; saved on exit"
        ));
    }

    /// Measure the terminal's real graphics cell size.
    ///
    /// Must be called once the alternate screen is active but before any
    /// input reader exists, since it writes a probe and reads the reply.
    /// Skipped when the user pinned `cell_px` themselves.
    pub fn calibrate_cells(&mut self) {
        if self.cell_source == crate::art::terminal::CellSource::Config {
            return;
        }
        // Kitty needs no cell size, so do not probe when it is in play.
        if self.graphics == Graphics::Kitty || !self.cfg.cover_mode.uses_sixel(true) {
            return;
        }
        match crate::art::terminal::calibrate(self.cell_px) {
            Some((w, h)) => {
                self.cell_px = (w, h);
                self.cell_source = crate::art::terminal::CellSource::Calibrated;
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
            None => {
                self.graphics = Graphics::resolve(&self.cfg, self.cell_source);
            }
        }
        if self.graphics == Graphics::None
            && self.cfg.cover_mode == crate::config::CoverMode::Auto
            && crate::art::terminal::terminal_supports_sixel()
        {
            if let Some(mux) = crate::art::terminal::multiplexer() {
                self.notify(format!(
                    "cover: half-blocks — sixel renders at the wrong size under {mux} \
                     (zellij#3372). Press b for sixel, then [ / ] to resize."
                ));
            }
        }
        self.clear_cover_art();
    }

    /// True when the cover is drawn by a graphics protocol rather than into
    /// ratatui's buffer. Requires a loaded image -- there is no placeholder.
    ///
    /// An open overlay turns this off. A pixel image is not part of the cell
    /// grid, so nothing drawn into ratatui's buffer can cover it: the menu
    /// would render underneath. Falling back to block art for as long as the
    /// overlay is up keeps the cover visible and lets the menu sit on top.
    pub fn graphics_active(&self) -> bool {
        self.cover_visible
            && self.graphics != Graphics::None
            && self.cover.is_some()
            && !self.menu_open
    }

    /// Take the region that was blanked, if any, so the renderer can reset
    /// ratatui's view of it.
    pub fn take_stale_cover(&mut self) -> Option<ratatui::layout::Rect> {
        self.stale_cover.take()
    }

    /// Emit the cover with the active graphics protocol, if what is on screen
    /// is stale.
    ///
    /// Called after the ratatui frame, because ratatui has no concept of
    /// pixel graphics: the cells are reserved during draw and the image is
    /// written over them here.
    pub fn paint_graphics(&mut self, rect: ratatui::layout::Rect) -> std::io::Result<()> {
        use std::io::Write;

        let Some(cover) = &self.cover else {
            return Ok(());
        };
        let Some(video_id) = &self.cover_for else {
            return Ok(());
        };
        // Nothing changed -- repainting would just burn bandwidth and flicker.
        if self
            .art_on_screen
            .as_ref()
            .is_some_and(|(id, r, kind)| id == video_id && *r == rect && *kind == self.graphics)
        {
            return Ok(());
        }

        let payload = match self.graphics {
            Graphics::Kitty => {
                // The terminal scales to the cell box, so send a fixed decent
                // resolution rather than trying to compute pixel dimensions.
                const MAX_PX: u32 = 512;
                let src = cover.source.as_ref();
                let scaled = if src.width() > MAX_PX || src.height() > MAX_PX {
                    image::imageops::resize(
                        src,
                        MAX_PX,
                        MAX_PX,
                        image::imageops::FilterType::Lanczos3,
                    )
                } else {
                    src.clone()
                };
                match crate::art::kitty::draw(&scaled, rect.width, rect.height) {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
            Graphics::Sixel => {
                let (cw, ch) = self.cell_px;
                let px_w = rect.width as u32 * cw as u32;
                let px_h = rect.height as u32 * ch as u32;
                if px_w == 0 || px_h == 0 {
                    return Ok(());
                }
                let resized = image::imageops::resize(
                    cover.source.as_ref(),
                    px_w,
                    px_h,
                    image::imageops::FilterType::Lanczos3,
                );
                crate::art::sixel::encode(&resized)
            }
            Graphics::None => return Ok(()),
        };

        let mut out = std::io::stdout().lock();
        // Clear the box before drawing into it. Both protocols keep the
        // image's aspect ratio, so it rarely fills the reserved cells exactly,
        // and the leftover margin is skipped by ratatui's diff -- whatever the
        // previous view left there would otherwise show through beside the
        // cover.
        out.write_all(blank_region(rect).as_bytes())?;
        if self.graphics == Graphics::Kitty {
            // Placements are terminal-side objects keyed by id, so drop the
            // old one rather than stacking a second on top of it.
            out.write_all(crate::art::kitty::delete().as_bytes())?;
        }
        // Both protocols draw from the cursor, so park it at the top-left of
        // the reserved area. Rows and columns are 1-based in CUP.
        write!(out, "\x1b[{};{}H", rect.y + 1, rect.x + 1)?;
        out.write_all(payload.as_bytes())?;
        out.flush()?;

        self.art_on_screen = Some((video_id.clone(), rect, self.graphics));
        Ok(())
    }

    /// Remove whatever image is on screen.
    ///
    /// Both protocols need explicit removal, for different reasons. A kitty
    /// placement belongs to the terminal rather than the cell grid, so
    /// drawing text over it does nothing -- it needs a delete command. Sixel
    /// pixels do live in the grid, but the cells were marked as skipped and
    /// so still hold their previous contents; if the next frame happens to
    /// draw the same thing there, ratatui's diff emits nothing and the art
    /// stays put. So the region is blanked directly rather than relying on
    /// the diff.
    ///
    /// Deliberately not `Terminal::clear()`: that queries the cursor position
    /// and blocks on the reply, which races the input reader for stdin.
    pub fn clear_cover_art(&mut self) {
        use std::io::Write;
        let Some((_, rect, kind)) = self.art_on_screen.take() else {
            return;
        };
        let mut out = std::io::stdout().lock();
        if kind == Graphics::Kitty {
            let _ = out.write_all(crate::art::kitty::delete().as_bytes());
        }
        // Blank the cells the image covered, and tell ratatui they are blank
        // so its next diff agrees with what is actually on screen.
        let _ = out.write_all(blank_region(rect).as_bytes());
        let _ = out.flush();
        self.stale_cover = Some(rect);
    }
}

/// Cursor-addressed spaces covering every cell of `rect`.
///
/// Used wherever the cell grid and the terminal have to be brought back into
/// agreement by hand, because ratatui's diff will not do it for cells that
/// were reserved for pixel graphics.
fn blank_region(rect: ratatui::layout::Rect) -> String {
    let blanks = " ".repeat(rect.width as usize);
    let mut out = String::new();
    for row in 0..rect.height {
        // CUP is 1-based.
        out.push_str(&format!(
            "\x1b[{};{}H{}",
            rect.y + row + 1,
            rect.x + 1,
            blanks
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::blank_region;
    use ratatui::layout::Rect;

    #[test]
    fn blanking_covers_every_reserved_cell() {
        let out = blank_region(Rect::new(4, 2, 3, 2));
        // One cursor move per row, 1-based, each followed by a full row of
        // spaces.
        assert_eq!(out, "\x1b[3;5H   \x1b[4;5H   ");
    }

    #[test]
    fn an_empty_region_emits_nothing() {
        assert!(blank_region(Rect::new(0, 0, 0, 0)).is_empty());
        assert!(blank_region(Rect::new(1, 1, 8, 0)).is_empty());
    }
}
