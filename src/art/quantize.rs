//! Adaptive colour reduction, shared by both graphics protocols.
//!
//! Sixel addresses colours through a register table, and the kitty path sends
//! an indexed PNG; both need the same median-cut palette and nearest-colour
//! lookup, so it lives here rather than inside either one.

/// Palette size. libsixel defaults to 256; 128 adaptive entries look at
/// least as good as a 216-entry fixed cube while emitting far less data,
/// because each six-row band then touches fewer distinct colours and the
/// run-length encoding has longer runs to work with.
pub const PALETTE_SIZE: usize = 128;

/// Side of the RGB lookup cube used to map pixels onto the palette. 32 gives
/// 32k entries, cheap to build once and O(1) per pixel afterwards.
const LUT_BITS: u32 = 5;
const LUT_SIDE: usize = 1 << LUT_BITS;

/// Build an adaptive palette by median cut.
///
/// Repeatedly split the bucket with the widest channel spread at its median,
/// which concentrates palette entries where the image actually has detail --
/// the whole reason this beats a fixed cube at the same entry count.
pub fn median_cut(samples: &mut Vec<[u8; 3]>, n: usize) -> Vec<[u8; 3]> {
    if samples.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut buckets: Vec<Vec<[u8; 3]>> = vec![std::mem::take(samples)];

    while buckets.len() < n {
        // Find the bucket worth splitting: widest single-channel range.
        let mut best = None;
        let mut best_range = 0u16;
        let mut best_channel = 0usize;
        for (i, b) in buckets.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..3 {
                let (mut lo, mut hi) = (255u8, 0u8);
                for p in b.iter() {
                    lo = lo.min(p[ch]);
                    hi = hi.max(p[ch]);
                }
                let range = hi as u16 - lo as u16;
                if range > best_range {
                    best_range = range;
                    best = Some(i);
                    best_channel = ch;
                }
            }
        }
        let Some(i) = best else { break };
        if best_range == 0 {
            break;
        }

        let mut bucket = buckets.swap_remove(i);
        bucket.sort_unstable_by_key(|p| p[best_channel]);
        let mid = bucket.len() / 2;
        let upper = bucket.split_off(mid);
        buckets.push(bucket);
        buckets.push(upper);
    }

    buckets
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let n = b.len() as u32;
            let sum = b.iter().fold([0u32; 3], |mut acc, p| {
                acc[0] += p[0] as u32;
                acc[1] += p[1] as u32;
                acc[2] += p[2] as u32;
                acc
            });
            [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
        })
        .collect()
}

/// Nearest-palette-entry lookup table over a coarse RGB cube.
pub fn build_lut(palette: &[[u8; 3]]) -> Vec<u8> {
    let shift = 8 - LUT_BITS;
    let mut lut = vec![0u8; LUT_SIDE * LUT_SIDE * LUT_SIDE];
    for r in 0..LUT_SIDE {
        for g in 0..LUT_SIDE {
            for b in 0..LUT_SIDE {
                // Centre of this LUT cell, in 0..255.
                let target = [
                    ((r as u32) << shift | (1 << shift) >> 1) as i32,
                    ((g as u32) << shift | (1 << shift) >> 1) as i32,
                    ((b as u32) << shift | (1 << shift) >> 1) as i32,
                ];
                let mut best = 0u8;
                let mut best_d = i32::MAX;
                for (i, p) in palette.iter().enumerate() {
                    let d = (p[0] as i32 - target[0]).pow(2)
                        + (p[1] as i32 - target[1]).pow(2)
                        + (p[2] as i32 - target[2]).pow(2);
                    if d < best_d {
                        best_d = d;
                        best = i as u8;
                    }
                }
                lut[(r << (LUT_BITS * 2)) | (g << LUT_BITS) | b] = best;
            }
        }
    }
    lut
}

#[inline]
pub fn lut_index(p: &[u8; 3]) -> usize {
    let shift = 8 - LUT_BITS;
    ((p[0] as usize >> shift) << (LUT_BITS * 2))
        | ((p[1] as usize >> shift) << LUT_BITS)
        | (p[2] as usize >> shift)
}
