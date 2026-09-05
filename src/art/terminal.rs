//! Terminal capability probing: cell size, multiplexer detection, and the
//! raw-tty plumbing the queries need.
//!
//! Kept apart from any one graphics protocol: both sixel and the kitty
//! capability query need the tty helpers, while only sixel needs the cell
//! size at all.

/// Where a cell size came from. Sixel needs the real cell size to scale an
/// image correctly, and a wrong value overflows the reserved area -- which
/// inside a multiplexer means painting over other panes. So the source is
/// tracked, and `auto` mode only enables sixel when it is trustworthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    /// Set explicitly in the config.
    Config,
    /// Measured by drawing a probe image and reading the cursor back. The
    /// only source that reflects what the terminal actually does with sixel.
    Calibrated,
    /// The terminal answered a CSI 16 t query. Good for the *aspect ratio*,
    /// but not for absolute scale: WezTerm on HiDPI answers in device pixels
    /// while laying sixel out in logical pixels, so the value can be a whole
    /// scale factor too large. Never trusted on its own.
    Query,
    /// Derived from the tty window size. Unreliable under multiplexers, which
    /// may report the outer window's pixels alongside the pane's cell count.
    Ioctl,
    /// Nothing usable; a conventional guess.
    Fallback,
}

impl CellSource {
    /// Only a definitive answer justifies drawing pixel graphics unasked.
    ///
    /// A `CSI 16 t` report is not enough on its own: it can be a whole scale
    /// factor off, and being too large means the cover paints over the track
    /// title. Only a pinned value or a measurement qualifies.
    pub fn is_trustworthy(self) -> bool {
        matches!(self, CellSource::Config | CellSource::Calibrated)
    }
}

const FALLBACK_CELL: (u16, u16) = (8, 16);

/// A cell narrower than 4px or taller than 64px is not a real font metric,
/// and neither is a wildly non-vertical aspect ratio.
fn plausible(cw: u16, ch: u16) -> bool {
    if !(4..=32).contains(&cw) || !(8..=64).contains(&ch) {
        return false;
    }
    let ratio = ch as f32 / cw as f32;
    (1.2..=3.5).contains(&ratio)
}

/// Resolve the cell size, preferring the most reliable source available.
pub fn detect_cell_size(config: Option<(u16, u16)>) -> ((u16, u16), CellSource) {
    if let Some((w, h)) = config {
        if w > 0 && h > 0 {
            return ((w, h), CellSource::Config);
        }
    }
    if let Some((w, h)) = query_cell_size() {
        if plausible(w, h) {
            return ((w, h), CellSource::Query);
        }
    }
    if let Some((w, h)) = ioctl_cell_size() {
        if plausible(w, h) {
            return ((w, h), CellSource::Ioctl);
        }
    }
    (FALLBACK_CELL, CellSource::Fallback)
}

/// Cell size from the tty window size, when the terminal fills it in.
fn ioctl_cell_size() -> Option<(u16, u16)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) != 0 {
            return None;
        }
        if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 || ws.ws_col == 0 || ws.ws_row == 0 {
            return None;
        }
        Some((ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row))
    }
}

/// Measure the cell size the terminal actually uses for *graphics*.
///
/// Reported cell sizes cannot be trusted for sixel: WezTerm on a HiDPI
/// display answers `CSI 16 t` in device pixels but lays sixel out in logical
/// pixels, so the reported value is the scale factor too large and the image
/// overflows by exactly that factor.
///
/// So measure it instead. Sixel advances the cursor by the number of rows the
/// image covered, so drawing a strip of known pixel height and reading the
/// cursor back gives the true rows-per-pixel. The width is then derived from
/// the reported aspect ratio, which is scale-invariant because both reported
/// numbers carry the same factor.
///
/// Must run on a quiet terminal, before any input reader is started.
pub fn calibrate(reported: (u16, u16)) -> Option<(u16, u16)> {
    use super::sixel::encode;
    // A taller probe measures more precisely, because the row count is
    // quantised: at 120px and a 25px cell the answer is only known to within
    // 20%, which is enough to overflow the layout. Bound it by the smallest
    // plausible cell so it can never scroll the screen, since scrolling
    // silently corrupts the reading.
    let rows_available = crossterm::terminal::size().map(|(_, r)| r).unwrap_or(24);
    let probe_px: u32 = (rows_available.saturating_sub(2) as u32 * 8)
        .min(reported.1 as u32 * 8)
        .clamp(96, 480);

    let mut tty = open_tty()?;
    let raw = RawMode::enable(&tty)?;

    let result = (|| -> Option<(u16, u16)> {
        use std::io::Write;
        // Park at the top-left before drawing. Measuring from the bottom of a
        // full screen makes the image scroll, and the cursor row then
        // saturates at the last line -- under-counting rows and so
        // over-estimating the cell height by whatever the overflow was.
        tty.write_all(b"\x1b7\x1b[1;1H").ok()?;
        tty.flush().ok()?;

        let strip = image::RgbImage::new(2, probe_px);
        let payload = encode(&strip);
        tty.write_all(payload.as_bytes()).ok()?;
        tty.flush().ok()?;
        let after = cursor_row(&mut tty)?;

        // Erase whatever the probe drew, then put the cursor back.
        tty.write_all(b"\x1b[1;1H\x1b[J\x1b8").ok()?;
        tty.flush().ok()?;

        // Started at row 1, so the advance is `after - 1`.
        let rows = after.saturating_sub(1);
        if rows == 0 {
            // Nothing was drawn, so sixel is not really supported here.
            return None;
        }
        // Sixel leaves the cursor on the row *containing* the bottom of the
        // image, so the advance is floor(height / cell), which means the true
        // cell height lies in (probe/(rows+1), probe/rows]. Take the lower
        // bound: under-estimating leaves a small gap below the cover, while
        // over-estimating paints the image over the track title.
        let cell_h = (probe_px as f32 / (rows + 1) as f32).floor().max(1.0) as u16;
        // Preserve the reported aspect; only the absolute scale was suspect.
        let ratio = reported.0 as f32 / reported.1.max(1) as f32;
        let cell_w = (cell_h as f32 * ratio).round().max(1.0) as u16;
        if plausible(cell_w, cell_h) {
            Some((cell_w, cell_h))
        } else {
            None
        }
    })();

    drop(raw);
    result
}

/// Ask the terminal for the cursor row via `CSI 6 n`, which answers
/// `CSI <row> ; <col> R`.
fn cursor_row(tty: &mut std::fs::File) -> Option<u16> {
    use std::io::Write;
    tty.write_all(b"\x1b[6n").ok()?;
    tty.flush().ok()?;
    let reply = read_reply(tty, b'R', 300)?;
    parse_cursor_row(&reply)
}

fn parse_cursor_row(bytes: &[u8]) -> Option<u16> {
    let s = std::str::from_utf8(bytes).ok()?;
    let start = s.rfind("\u{1b}[")?;
    let body = &s[start + 2..];
    let end = body.find('R')?;
    body[..end].split(';').next()?.trim().parse().ok()
}

/// Read from the tty until `terminator` arrives or the deadline passes.
pub fn read_reply(tty: &mut std::fs::File, terminator: u8, millis: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    let fd = tty.as_raw_fd();
    let mut buf = [0u8; 64];
    let mut got = 0usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
    while std::time::Instant::now() < deadline && got < buf.len() {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as i32;
        if unsafe { libc::poll(&mut poll, 1, remaining.max(1)) } <= 0 {
            break;
        }
        match tty.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => break,
        }
        if buf[..got].contains(&terminator) {
            break;
        }
    }
    if got == 0 {
        None
    } else {
        Some(buf[..got].to_vec())
    }
}

pub fn open_tty() -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()
}

/// Puts the tty in raw mode and restores the previous settings on drop.
pub struct RawMode {
    fd: i32,
    saved: libc::termios,
}

impl RawMode {
    pub fn enable(tty: &std::fs::File) -> Option<Self> {
        use std::os::unix::io::AsRawFd;
        let fd = tty.as_raw_fd();
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return None;
        }
        let mut raw = saved;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        Some(Self { fd, saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
    }
}

/// Ask the terminal for its cell size with `CSI 16 t`, which answers
/// `CSI 6 ; height ; width t`.
///
/// This is the only way to get a correct answer through a multiplexer, and it
/// must run before the TUI takes over the terminal.
fn query_cell_size() -> Option<(u16, u16)> {
    let mut tty = open_tty()?;
    let raw = RawMode::enable(&tty)?;
    let result = (|| -> Option<(u16, u16)> {
        use std::io::Write;
        tty.write_all(b"\x1b[16t").ok()?;
        tty.flush().ok()?;
        // Terminals that do not implement this never answer, so the read must
        // time out rather than hang startup.
        let reply = read_reply(&mut tty, b't', 200)?;
        parse_cell_report(&reply)
    })();
    drop(raw);
    result
}

/// Parse `CSI 6 ; <height> ; <width> t`.
fn parse_cell_report(bytes: &[u8]) -> Option<(u16, u16)> {
    let s = std::str::from_utf8(bytes).ok()?;
    // Find the report even if other input arrived alongside it.
    let start = s.find("\x1b[6;").or_else(|| s.find("\u{1b}[6;"))?;
    let rest = &s[start..];
    let body = rest.strip_prefix("\u{1b}[6;")?;
    let end = body.find('t')?;
    let mut parts = body[..end].split(';');
    let height: u16 = parts.next()?.trim().parse().ok()?;
    let width: u16 = parts.next()?.trim().parse().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Name of the terminal multiplexer in use, if any.
pub fn multiplexer() -> Option<&'static str> {
    if std::env::var_os("ZELLIJ").is_some() {
        return Some("zellij");
    }
    if std::env::var_os("TMUX").is_some() {
        return Some("tmux");
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term.starts_with("screen") || term.starts_with("tmux") {
        return Some("screen/tmux");
    }
    None
}

/// Should `auto` draw sixel here?
///
/// Sixel works correctly in a bare terminal. Inside zellij it does not:
/// zellij-org/zellij#3372 (open since 0.35.0) renders sixel at double height,
/// and nothing the terminal reports reveals that, so the image overflows its
/// reserved area and bleeds across panes. Half-blocks are always correctly
/// sized, so a multiplexer gets those unless the user has explicitly pinned a
/// cell size to compensate.
pub fn sixel_recommended(source: CellSource) -> bool {
    if !terminal_supports_sixel() || !source.is_trustworthy() {
        return false;
    }
    match multiplexer() {
        // A pinned size means the user has tuned it themselves.
        Some(_) => source == CellSource::Config,
        None => true,
    }
}

/// Whether this terminal is known to render sixel. There is a DA1 query that
/// would answer definitively, but matching the environment covers the common
/// terminals and the config can always override.
pub fn terminal_supports_sixel() -> bool {
    if let Ok(p) = std::env::var("TERM_PROGRAM") {
        let p = p.to_ascii_lowercase();
        if ["wezterm", "mlterm", "contour", "iterm.app"]
            .iter()
            .any(|k| p.contains(k))
        {
            return true;
        }
    }
    let term = std::env::var("TERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["sixel", "foot", "mlterm", "yaft", "contour"]
        .iter()
        .any(|k| term.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_cell_size_is_always_plausible() {
        let ((w, h), _src) = detect_cell_size(None);
        assert!(plausible(w, h), "got {w}x{h}");
    }

    #[test]
    fn config_cell_size_wins_over_detection() {
        let ((w, h), src) = detect_cell_size(Some((11, 23)));
        assert_eq!((w, h), (11, 23));
        assert_eq!(src, CellSource::Config);
        assert!(src.is_trustworthy());
    }

    #[test]
    fn zero_config_cell_size_falls_through_to_detection() {
        let (_, src) = detect_cell_size(Some((0, 0)));
        assert_ne!(src, CellSource::Config);
    }

    #[test]
    fn implausible_cell_sizes_are_rejected() {
        // Multiplexers can report the outer window's pixels with a pane's
        // cell count, which yields nonsense like a 40px-wide cell.
        assert!(!plausible(40, 25), "absurdly wide cell must be rejected");
        assert!(!plausible(8, 4), "cell shorter than it is wide");
        assert!(!plausible(2, 8), "cell too narrow");
        assert!(!plausible(16, 100), "cell too tall");
        // Real-world values must pass.
        assert!(plausible(8, 16));
        assert!(plausible(16, 32));
        assert!(plausible(12, 24));
        assert!(plausible(10, 21));
    }

    #[test]
    fn only_pinned_or_measured_cell_sizes_are_trusted() {
        // A reported size that is too large makes the cover overlap the
        // track title, so reports never enable sixel by themselves.
        assert!(!CellSource::Query.is_trustworthy());
        assert!(!CellSource::Ioctl.is_trustworthy());
        assert!(!CellSource::Fallback.is_trustworthy());
        assert!(CellSource::Config.is_trustworthy());
        assert!(CellSource::Calibrated.is_trustworthy());
    }

    #[test]
    fn a_multiplexer_only_gets_sixel_when_the_size_is_pinned() {
        // Guards the zellij double-height regression: a detected size is
        // fine in a bare terminal but must not enable sixel under a
        // multiplexer, where it renders at the wrong scale.
        let in_mux = multiplexer().is_some();
        if in_mux {
            assert!(!sixel_recommended(CellSource::Calibrated));
            assert!(!sixel_recommended(CellSource::Query));
        }
        // A guessed size never qualifies, multiplexer or not.
        assert!(!sixel_recommended(CellSource::Fallback));
        assert!(!sixel_recommended(CellSource::Ioctl));
    }

    #[test]
    fn parses_a_cursor_position_report() {
        // CSI <row> ; <col> R
        assert_eq!(parse_cursor_row(b"\x1b[13;1R"), Some(13));
        assert_eq!(parse_cursor_row(b"\x1b[1;1R"), Some(1));
        // Takes the last report if several arrived.
        assert_eq!(parse_cursor_row(b"\x1b[3;1R\x1b[15;1R"), Some(15));
    }

    #[test]
    fn rejects_malformed_cursor_reports() {
        assert_eq!(parse_cursor_row(b""), None);
        assert_eq!(parse_cursor_row(b"\x1b[13;1"), None, "unterminated");
        assert_eq!(parse_cursor_row(b"garbage"), None);
    }

    #[test]
    fn parses_a_cell_size_report() {
        // CSI 6 ; height ; width t
        assert_eq!(parse_cell_report(b"\x1b[6;32;16t"), Some((16, 32)));
        // Tolerates surrounding noise.
        assert_eq!(parse_cell_report(b"junk\x1b[6;21;10tmore"), Some((10, 21)));
    }

    #[test]
    fn rejects_malformed_or_missing_cell_reports() {
        assert_eq!(parse_cell_report(b""), None);
        assert_eq!(parse_cell_report(b"\x1b[6;32"), None, "unterminated");
        assert_eq!(
            parse_cell_report(b"\x1b[4;953;1383t"),
            None,
            "wrong report type"
        );
        assert_eq!(parse_cell_report(b"\x1b[6;0;0t"), None, "zero size");
    }
}
