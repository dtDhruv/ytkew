//! Album-art colour extraction.
//!
//! kew derives its whole theme from the cover, which is most of why it looks
//! good. We do the same: k-means over the cover's pixels, then sort the
//! centroids by luminance so callers get a predictable dark/mid/bright ramp.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn to_color(self) -> Color {
        Color::Rgb(self.0, self.1, self.2)
    }

    fn luminance(self) -> f32 {
        // Rec. 709 luma.
        0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32
    }

    fn dist2(self, o: Rgb) -> f32 {
        let d = (
            self.0 as f32 - o.0 as f32,
            self.1 as f32 - o.1 as f32,
            self.2 as f32 - o.2 as f32,
        );
        d.0 * d.0 + d.1 * d.1 + d.2 * d.2
    }

    /// Push a colour up to a usable brightness. Covers are often near-black,
    /// and an accent derived from one would be invisible against the terminal.
    pub fn ensure_visible(self, min_lum: f32) -> Rgb {
        let lum = self.luminance();
        if lum >= min_lum || lum < 1.0 {
            return if lum < 1.0 { Rgb(140, 140, 140) } else { self };
        }
        let scale = min_lum / lum.max(1.0);
        Rgb(
            (self.0 as f32 * scale).min(255.0) as u8,
            (self.1 as f32 * scale).min(255.0) as u8,
            (self.2 as f32 * scale).min(255.0) as u8,
        )
    }
}

/// Three representative colours, darkest first.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: Rgb,
    pub mid: Rgb,
    pub bright: Rgb,
}

impl Palette {
    /// The colour used for titles, the progress bar and visualizer peaks.
    pub fn accent(&self) -> Rgb {
        self.bright.ensure_visible(110.0)
    }

    /// A dimmer companion for secondary text.
    pub fn secondary(&self) -> Rgb {
        self.mid.ensure_visible(80.0)
    }

    /// Ordinary list text.
    ///
    /// Rows used to be drawn in the border colour, which is fine on a solid
    /// dark background and close to unreadable on a translucent or blurred
    /// one -- and a cover-derived palette can make that colour nearly black.
    /// This keeps a floor under it.
    pub fn body(&self) -> Rgb {
        self.mid.ensure_visible(105.0)
    }

    /// Secondary detail beside body text: artist columns, sublabels, hints.
    /// Dimmer than `body`, but still above the point where it disappears.
    pub fn muted(&self) -> Rgb {
        self.dark.ensure_visible(78.0)
    }

    /// Borders, rules and scrollbar tracks. Meant to recede, but not to
    /// vanish entirely on a light or transparent background.
    pub fn chrome(&self) -> Rgb {
        self.dark.ensure_visible(52.0)
    }

    /// Vertical gradient for the visualizer: dark at the base, accent on top,
    /// which is what makes kew's spectrum read as a single object.
    pub fn gradient(&self, steps: usize) -> Vec<Rgb> {
        if steps == 0 {
            return Vec::new();
        }
        let (a, b) = (self.secondary(), self.accent());
        (0..steps)
            .map(|i| {
                let t = if steps == 1 {
                    1.0
                } else {
                    i as f32 / (steps - 1) as f32
                };
                Rgb(
                    (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
                    (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
                    (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
                )
            })
            .collect()
    }
}

impl Default for Palette {
    /// kew's fallback accent is ANSI 6 (cyan).
    fn default() -> Self {
        Self::from_ansi(6)
    }
}

/// RGB for an xterm-256 palette index, so a configured `accent_color` means
/// the same thing here as it does in kew.
pub fn ansi_rgb(index: u8) -> Rgb {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (170, 0, 0),
        (0, 170, 0),
        (170, 85, 0),
        (0, 0, 170),
        (170, 0, 170),
        (0, 170, 170),
        (170, 170, 170),
        (85, 85, 85),
        (255, 85, 85),
        (85, 255, 85),
        (255, 255, 85),
        (85, 85, 255),
        (255, 85, 255),
        (85, 255, 255),
        (255, 255, 255),
    ];
    match index {
        0..=15 => {
            let (r, g, b) = BASE[index as usize];
            Rgb(r, g, b)
        }
        16..=231 => {
            // 6x6x6 colour cube.
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let i = index as usize - 16;
            Rgb(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
        }
        _ => {
            // 24-step greyscale ramp.
            let v = 8u16 + 10 * (index as u16 - 232);
            let v = v.min(255) as u8;
            Rgb(v, v, v)
        }
    }
}

impl Palette {
    /// Build a full dark/mid/bright ramp from one accent colour, for when
    /// there is no cover to sample or cover-theming is switched off.
    pub fn from_accent(accent: Rgb) -> Self {
        let scale = |f: f32| {
            Rgb(
                (accent.0 as f32 * f).min(255.0) as u8,
                (accent.1 as f32 * f).min(255.0) as u8,
                (accent.2 as f32 * f).min(255.0) as u8,
            )
        };
        Self {
            dark: scale(0.30),
            mid: scale(0.62),
            bright: accent,
        }
    }

    pub fn from_ansi(index: u8) -> Self {
        Self::from_accent(ansi_rgb(index).ensure_visible(110.0))
    }
}

/// k-means with a fixed iteration count. Deterministic seeding (spread across
/// the sorted-by-luminance sample set) keeps the theme stable across restarts
/// instead of flickering between runs.
pub fn extract(pixels: &[Rgb], k: usize) -> Palette {
    let k = k.max(1);
    // Ignore near-transparent-ish extremes that would drag centroids around.
    let samples: Vec<Rgb> = pixels
        .iter()
        .copied()
        .filter(|p| {
            let l = p.luminance();
            l > 8.0 && l < 250.0
        })
        .collect();
    let samples = if samples.len() < k {
        pixels.to_vec()
    } else {
        samples
    };
    if samples.is_empty() {
        return Palette::default();
    }

    let mut sorted = samples.clone();
    sorted.sort_by(|a, b| a.luminance().total_cmp(&b.luminance()));
    let mut centroids: Vec<Rgb> = (0..k)
        .map(|i| sorted[(i * (sorted.len() - 1)) / k.max(1)])
        .collect();

    for _ in 0..12 {
        let mut sums = vec![(0f64, 0f64, 0f64, 0usize); k];
        for p in &samples {
            let (mut best, mut best_d) = (0usize, f32::MAX);
            for (i, c) in centroids.iter().enumerate() {
                let d = p.dist2(*c);
                if d < best_d {
                    best_d = d;
                    best = i;
                }
            }
            let s = &mut sums[best];
            s.0 += p.0 as f64;
            s.1 += p.1 as f64;
            s.2 += p.2 as f64;
            s.3 += 1;
        }
        for (i, s) in sums.iter().enumerate() {
            if s.3 > 0 {
                centroids[i] = Rgb(
                    (s.0 / s.3 as f64) as u8,
                    (s.1 / s.3 as f64) as u8,
                    (s.2 / s.3 as f64) as u8,
                );
            }
        }
    }

    centroids.sort_by(|a, b| a.luminance().total_cmp(&b.luminance()));
    let pick = |i: usize| centroids[i.min(centroids.len() - 1)];
    Palette {
        dark: pick(0),
        mid: pick(centroids.len() / 2),
        bright: pick(centroids.len() - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Rgb) -> f32 {
        0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32
    }

    #[test]
    fn text_tiers_stay_readable_even_from_a_near_black_palette() {
        // A cover of a dark album art can produce exactly this.
        let p = Palette {
            dark: Rgb(6, 5, 8),
            mid: Rgb(20, 18, 24),
            bright: Rgb(40, 36, 48),
        };
        assert!(lum(p.body()) >= 100.0, "body {:?}", p.body());
        assert!(lum(p.muted()) >= 70.0, "muted {:?}", p.muted());
        assert!(lum(p.chrome()) >= 45.0, "chrome {:?}", p.chrome());
    }

    #[test]
    fn the_tiers_stay_in_order() {
        for p in [
            Palette {
                dark: Rgb(0x50, 0x49, 0x45),
                mid: Rgb(0xd5, 0xc4, 0xa1),
                bright: Rgb(0xfa, 0xbd, 0x2f),
            },
            Palette {
                dark: Rgb(6, 5, 8),
                mid: Rgb(20, 18, 24),
                bright: Rgb(40, 36, 48),
            },
        ] {
            assert!(
                lum(p.chrome()) <= lum(p.muted()),
                "chrome must recede behind detail text"
            );
            assert!(
                lum(p.muted()) <= lum(p.body()),
                "detail must recede behind body text"
            );
        }
    }

    #[test]
    fn separates_two_obvious_clusters() {
        let mut px = vec![Rgb(10, 10, 10); 200];
        px.extend(vec![Rgb(240, 30, 30); 200]);
        let p = extract(&px, 3);
        assert!(p.dark.luminance() < p.bright.luminance());
        // The bright end should be recognisably the red cluster.
        assert!(p.bright.0 > p.bright.1 + 40);
    }

    #[test]
    fn survives_degenerate_input() {
        assert!(matches!(extract(&[], 3), Palette { .. }));
        let p = extract(&[Rgb(0, 0, 0)], 3);
        // A pure black cover must still yield a visible accent.
        assert!(p.accent().luminance() > 50.0);
    }

    #[test]
    fn ansi_indices_map_to_expected_rgb() {
        assert_eq!(ansi_rgb(6), Rgb(0, 170, 170), "ANSI 6 is cyan");
        assert_eq!(ansi_rgb(1), Rgb(170, 0, 0), "ANSI 1 is red");
        // Cube corner: index 231 is the brightest cube entry.
        assert_eq!(ansi_rgb(231), Rgb(255, 255, 255));
        // Greyscale ramp start and end.
        assert_eq!(ansi_rgb(232), Rgb(8, 8, 8));
        assert_eq!(ansi_rgb(255), Rgb(238, 238, 238));
    }

    #[test]
    fn accent_config_drives_the_palette() {
        // A red accent must produce a red-dominant, visible accent.
        let p = Palette::from_ansi(1);
        let a = p.accent();
        assert!(a.0 > a.1 && a.0 > a.2, "accent should stay red, got {a:?}");
        assert!(p.dark.luminance() < p.bright.luminance());
        // And the default still matches kew's cyan.
        let d = Palette::default().accent();
        assert!(
            d.1 > d.0 && d.2 > d.0,
            "default should be cyan-ish, got {d:?}"
        );
    }

    #[test]
    fn gradient_runs_dark_to_bright() {
        let p = Palette::default();
        let g = p.gradient(6);
        assert_eq!(g.len(), 6);
        assert!(g[0].luminance() < g[5].luminance());
        assert!(p.gradient(0).is_empty());
        assert_eq!(p.gradient(1).len(), 1);
    }
}
