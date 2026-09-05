//! Album art: fetch, cache on disk, and render into terminal cells.
//!
//! Rendering uses the upper-half-block trick -- each cell draws '▀' with the
//! top pixel as foreground and the bottom pixel as background, giving two
//! vertical pixels per cell. That keeps the cover inside ratatui's normal
//! buffer (unlike sixel, which would need out-of-band passthrough and its own
//! damage tracking) and it works on any truecolor terminal.

use crate::palette::{extract, Palette, Rgb};
use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::DynamicImage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// A cover rendered to a grid of half-block cells, plus the theme drawn from it.
#[derive(Clone, Debug)]
pub struct Cover {
    /// `cells[row][col]` = (top pixel, bottom pixel), for the block renderer.
    pub cells: Vec<Vec<(Rgb, Rgb)>>,
    pub palette: Palette,
    /// The decoded source, kept so the sixel path can resize to whatever
    /// pixel dimensions the cell grid actually works out to.
    pub source: std::sync::Arc<image::RgbImage>,
}


pub struct CoverLoader {
    cache: PathBuf,
    client: reqwest::Client,
}

impl CoverLoader {
    pub fn new(cache: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache);
        Self {
            cache,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(12))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Fetch the image, serving from disk when we've seen this URL before.
    pub async fn load(&self, url: &str) -> Result<DynamicImage> {
        let path = self.cache_path(url);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            if let Ok(img) = image::load_from_memory(&bytes) {
                return Ok(img);
            }
        }
        let bytes = self
            .client
            .get(url)
            .send()
            .await
            .context("fetching cover art")?
            .bytes()
            .await
            .context("reading cover art")?;
        // Best-effort cache write; a failure here is not worth surfacing.
        let _ = tokio::fs::write(&path, &bytes).await;
        image::load_from_memory(&bytes).context("decoding cover art")
    }

    /// Where a cover URL is cached on disk.
    pub fn cache_path(&self, url: &str) -> PathBuf {
        let mut h = DefaultHasher::new();
        url.hash(&mut h);
        self.cache.join(format!("{:016x}", h.finish()))
    }
}

/// Render `img` to at most `max_cols` x `rows` cells, preserving the image's
/// aspect ratio. Half-block pixels are roughly square, so a square cover wants
/// twice as many columns as rows.
pub fn render_fit(img: &DynamicImage, max_cols: u16, rows: u16) -> Cover {
    if rows == 0 || max_cols == 0 {
        return Cover {
            cells: Vec::new(),
            palette: Palette::default(),
            source: std::sync::Arc::new(img.to_rgb8()),
        };
    }
    let (iw, ih) = (img.width().max(1) as f32, img.height().max(1) as f32);
    let aspect = iw / ih;

    // Two pixels per cell vertically means `rows*2` pixels tall.
    let px_h = rows as u32 * 2;
    let px_w = ((px_h as f32) * aspect).round().max(1.0) as u32;
    let cols = px_w.min(max_cols as u32).max(1);
    // Recompute pixel width from the column budget we actually got.
    let px_w = cols;

    let small = img.resize_exact(px_w, px_h, FilterType::Lanczos3);
    let rgb = small.to_rgb8();

    let mut cells = Vec::with_capacity(rows as usize);
    let mut all_pixels = Vec::with_capacity((px_w * px_h) as usize);
    for row in 0..rows {
        let mut line = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            let y_top = row as u32 * 2;
            let y_bot = y_top + 1;
            let top = px(&rgb, col, y_top);
            let bot = px(&rgb, col, y_bot.min(px_h - 1));
            all_pixels.push(top);
            all_pixels.push(bot);
            line.push((top, bot));
        }
        cells.push(line);
    }

    let palette = extract(&all_pixels, 3);
    Cover {
        cells,
        palette,
        source: std::sync::Arc::new(img.to_rgb8()),
    }
}

fn px(img: &image::RgbImage, x: u32, y: u32) -> Rgb {
    let p = img.get_pixel(x.min(img.width() - 1), y.min(img.height() - 1));
    Rgb(p[0], p[1], p[2])
}


#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb as IRgb, RgbImage};

    fn test_image(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                // Left half red, right half blue, so we can check orientation.
                let c = if x < w / 2 {
                    IRgb([200, 20, 20])
                } else {
                    IRgb([20, 20, 200])
                };
                img.put_pixel(x, y, c);
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn square_cover_gets_twice_as_many_columns_as_rows() {
        let c = render_fit(&test_image(200, 200), 100, 10);
        assert_eq!(c.cells.len(), 10);
        assert_eq!(c.cells[0].len(), 20, "square art should be 2:1 in cells");
    }

    #[test]
    fn respects_a_narrow_column_budget() {
        let c = render_fit(&test_image(200, 200), 8, 10);
        assert_eq!(c.cells[0].len(), 8);
        assert_eq!(c.cells.len(), 10);
    }

    #[test]
    fn preserves_left_right_orientation() {
        let c = render_fit(&test_image(200, 200), 100, 8);
        let row = &c.cells[4];
        let left = row[1].0;
        let right = row[row.len() - 2].0;
        assert!(left.0 > left.2, "left edge should be reddish");
        assert!(right.2 > right.0, "right edge should be blueish");
    }

    #[test]
    fn zero_sized_region_is_empty_not_a_panic() {
        let c = render_fit(&test_image(64, 64), 0, 0);
        assert!(c.cells.is_empty());
    }
}
