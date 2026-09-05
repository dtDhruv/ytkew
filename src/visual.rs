//! Spectrum visualizer.
//!
//! mpv gives us no access to the samples it is decoding, so instead of trying
//! to tap the player we capture the audio sink's monitor -- the same approach
//! cava takes. A background thread reads raw PCM from `pw-cat`, runs an FFT,
//! folds the bins into log-spaced bands and stores smoothed magnitudes for the
//! renderer to read. If any of that fails the visualizer just stays flat; it
//! must never take the player down with it.

use rustfft::{num_complex::Complex32, FftPlanner};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const FFT_SIZE: usize = 2048;
const SAMPLE_RATE: f32 = 48000.0;
pub const MAX_BANDS: usize = 160;
/// Bass below this is mostly rumble; treble above it is mostly air.
const MIN_FREQ: f32 = 45.0;
const MAX_FREQ: f32 = 16000.0;
/// How fast a bar falls when the music stops pushing it up.
const DECAY: f32 = 0.86;

pub struct Visualizer {
    bands: Arc<Mutex<Vec<f32>>>,
    running: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    pub available: bool,
}

/// Take a lock, recovering it if a thread panicked while holding it.
///
/// The capture thread is best-effort by design -- if anything about it fails
/// the visualizer just stays flat -- so a poisoned mutex should leave the
/// spectrum stale, not take the player down with it.
fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Visualizer {
    /// Start capturing. Returns a disabled-but-harmless visualizer when the
    /// audio server or `pw-cat` is not there.
    pub fn start() -> Self {
        let bands = Arc::new(Mutex::new(vec![0.0f32; MAX_BANDS]));
        let running = Arc::new(AtomicBool::new(true));
        let child = Arc::new(Mutex::new(None));

        let Some(sink) = default_sink() else {
            return Self {
                bands,
                running,
                child,
                available: false,
            };
        };

        // `stream.capture.sink=true` is what turns a record stream into a
        // monitor of the sink's output rather than a microphone capture.
        let spawned = Command::new("pw-cat")
            .args([
                "--record",
                "--format=s16",
                "--rate=48000",
                "--channels=2",
                "--latency=1024/48000",
                &format!("--target={sink}"),
                "-P",
                "{ stream.capture.sink=true }",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn();

        // No audio server, or pw-cat is not installed: the visualizer stays
        // flat and everything else carries on.
        let Ok(mut proc) = spawned else {
            return Self {
                bands,
                running,
                child,
                available: false,
            };
        };

        let Some(stdout) = proc.stdout.take() else {
            return Self {
                bands,
                running,
                child,
                available: false,
            };
        };
        *lock(&child) = Some(proc);

        let bands_w = bands.clone();
        let running_w = running.clone();
        std::thread::spawn(move || {
            capture_loop(stdout, bands_w, running_w);
        });

        Self {
            bands,
            running,
            child,
            available: true,
        }
    }

    /// Current magnitudes resampled to `n` bars, each 0.0..=1.0.
    pub fn bars(&self, n: usize) -> Vec<f32> {
        if n == 0 {
            return Vec::new();
        }
        let src = lock(&self.bands);
        (0..n)
            .map(|i| {
                // Average the slice of bands that maps onto this bar so wide
                // bars don't just sample one band and alias.
                let lo = i * MAX_BANDS / n;
                let hi = (((i + 1) * MAX_BANDS) / n).max(lo + 1).min(MAX_BANDS);
                let slice = &src[lo..hi];
                slice.iter().copied().fold(0.0f32, f32::max)
            })
            .collect()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(mut c) = lock(&self.child).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Visualizer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture_loop(mut stdout: impl Read, bands: Arc<Mutex<Vec<f32>>>, running: Arc<AtomicBool>) {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let window = blackman_harris(FFT_SIZE);
    let band_edges = log_band_edges(MAX_BANDS);

    // Interleaved stereo i16 -> mono f32 ring.
    let mut raw = vec![0u8; FFT_SIZE * 2 * 2];
    let mut ring = vec![0.0f32; FFT_SIZE];
    let mut smoothed = vec![0.0f32; MAX_BANDS];
    // Slowly-adapting gain, so quiet tracks still fill the bars and loud ones
    // don't clip everything to full height.
    let mut peak = 1e-4f32;

    while running.load(Ordering::Relaxed) {
        if stdout.read_exact(&mut raw).is_err() {
            break;
        }
        // Downmix to mono, oldest-to-newest.
        for (i, chunk) in raw.as_chunks::<4>().0.iter().enumerate() {
            if i >= FFT_SIZE {
                break;
            }
            let l = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([chunk[2], chunk[3]]) as f32 / 32768.0;
            ring[i] = (l + r) * 0.5;
        }

        let mut buf: Vec<Complex32> = ring
            .iter()
            .zip(&window)
            .map(|(s, w)| Complex32::new(s * w, 0.0))
            .collect();
        fft.process(&mut buf);

        // Magnitude per band: peak bin within the band's range.
        let mut frame = vec![0.0f32; MAX_BANDS];
        for b in 0..MAX_BANDS {
            let (lo, hi) = (band_edges[b], band_edges[b + 1]);
            let hi = hi.max(lo + 1).min(FFT_SIZE / 2);
            let m = buf[lo..hi.max(lo)]
                .iter()
                .map(|c| c.norm())
                .fold(0.0f32, f32::max);
            // Perceptual-ish compression; raw magnitudes are far too spiky.
            frame[b] = (1.0 + m).ln();
        }

        let frame_peak = frame.iter().copied().fold(0.0f32, f32::max);
        peak = if frame_peak > peak {
            frame_peak
        } else {
            // Decay the gain reference slowly so it tracks the track, not the beat.
            (peak * 0.995).max(1e-4)
        };

        for b in 0..MAX_BANDS {
            let v = (frame[b] / peak).clamp(0.0, 1.0);
            // Rise instantly, fall gradually -- the classic spectrum look.
            smoothed[b] = if v > smoothed[b] {
                v
            } else {
                smoothed[b] * DECAY
            };
        }

        if let Ok(mut out) = bands.lock() {
            out.copy_from_slice(&smoothed);
        }
    }

    // Let the bars fall to zero rather than freezing mid-air.
    if let Ok(mut out) = bands.lock() {
        out.iter_mut().for_each(|v| *v = 0.0);
    }
}

fn blackman_harris(n: usize) -> Vec<f32> {
    // Same window kew uses; low sidelobes keep neighbouring bands from bleeding.
    const A: [f32; 4] = [0.35875, 0.48829, 0.14128, 0.01168];
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            A[0] - A[1] * (2.0 * std::f32::consts::PI * t).cos()
                + A[2] * (4.0 * std::f32::consts::PI * t).cos()
                - A[3] * (6.0 * std::f32::consts::PI * t).cos()
        })
        .collect()
}

/// FFT bin index boundaries for log-spaced bands, so each bar covers roughly
/// equal musical distance instead of equal Hz.
fn log_band_edges(bands: usize) -> Vec<usize> {
    let bin_hz = SAMPLE_RATE / FFT_SIZE as f32;
    let ratio = (MAX_FREQ / MIN_FREQ).ln();
    (0..=bands)
        .map(|b| {
            let f = MIN_FREQ * (ratio * b as f32 / bands as f32).exp();
            ((f / bin_hz) as usize).min(FFT_SIZE / 2 - 1)
        })
        .collect()
}

/// Ask PipeWire for the default sink, falling back to the first one it lists.
fn default_sink() -> Option<String> {
    if let Ok(out) = Command::new("pw-metadata").args(["-n", "default"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.contains("default.audio.sink") {
                // value:'{"name":"alsa_output..."}'
                if let Some(i) = line.find("\"name\":\"") {
                    let rest = &line[i + 8..];
                    if let Some(j) = rest.find('"') {
                        return Some(rest[..j].to_string());
                    }
                }
            }
        }
    }

    let out = Command::new("pw-dump").output().ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json.as_array()?.iter().find_map(|o| {
        let props = o.get("info")?.get("props")?;
        if props.get("media.class")?.as_str()? == "Audio/Sink" {
            Some(props.get("node.name")?.as_str()?.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_symmetric_and_peaks_in_the_middle() {
        let w = blackman_harris(64);
        assert!((w[0]).abs() < 0.01, "window should start near zero");
        assert!(w[32] > 0.9, "window should peak near 1 in the middle");
        for i in 0..32 {
            assert!((w[i] - w[63 - i]).abs() < 1e-5, "should be symmetric");
        }
    }

    #[test]
    fn band_edges_are_monotonic_and_in_range() {
        let e = log_band_edges(MAX_BANDS);
        assert_eq!(e.len(), MAX_BANDS + 1);
        for i in 1..e.len() {
            assert!(e[i] >= e[i - 1], "edges must not go backwards");
        }
        assert!(*e.last().unwrap() < FFT_SIZE / 2);
    }

    #[test]
    fn bars_resample_without_panicking() {
        let v = Visualizer {
            bands: Arc::new(Mutex::new(vec![0.5; MAX_BANDS])),
            running: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            available: false,
        };
        for n in [1usize, 7, 40, 160, 200] {
            let b = v.bars(n);
            assert_eq!(b.len(), n);
            assert!(b.iter().all(|x| (0.0..=1.0).contains(x)));
        }
        assert!(v.bars(0).is_empty());
    }
}
