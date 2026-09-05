//! ytkew -- a terminal YouTube Music player.
//!
//! Structurally: `ytmapi-rs` for YouTube Music's internal API, mpv over its
//! JSON IPC socket for playback, ratatui for the interface, and a PipeWire
//! monitor tap for the spectrum visualizer. The look and keybindings follow
//! kew (https://github.com/ravachol/kew).

mod api;
mod app;
mod config;
mod cover;
mod kitty;
mod model;
mod mpris;
mod palette;
mod player;
mod queue;
mod sixel;
mod theme;
mod ui;
mod visual;

use anyhow::{Context, Result};
use app::{App, AppMsg};
use clap::Parser;
use cover::CoverLoader;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use player::Player;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Redraw cadence. The visualizer needs a steady tick; everything else is
/// event-driven.
const FRAME: Duration = Duration::from_millis(33);

#[derive(Parser, Debug)]
#[command(
    name = "ytkew",
    about = "A terminal YouTube Music player, in the spirit of kew",
    version
)]
struct Cli {
    /// Search terms -- plays the best match immediately, like `kew nirvana`.
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Set up credentials: `ytkew --auth cookie` or `ytkew --auth oauth`.
    #[arg(long, value_name = "METHOD")]
    auth: Option<String>,

    /// Report what the API can see with the current credentials.
    #[arg(long)]
    diagnose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg_dir = api::config_dir();

    if let Some(method) = cli.auth.as_deref() {
        return run_auth(method, &cfg_dir).await;
    }
    if cli.diagnose {
        return run_diagnose(&cfg_dir).await;
    }

    let cfg = config::Config::load(&cfg_dir);
    // Drop a commented default on first run; never overwrite an existing one.
    let _ = config::Config::write_default_if_missing(&cfg_dir);
    let mut state = config::State::load(&cfg_dir, &cfg);
    // A cell size tuned by eye outranks anything detected.
    let mut cfg = cfg;
    if cfg.cell_px == [0, 0] && state.cover_cell != [0, 0] {
        cfg.cell_px = state.cover_cell;
    }
    state.cover_cell = cfg.cell_px;
    let (api_handle, warning) = api::Api::connect(&cfg_dir).await;
    let api = Arc::new(api_handle);

    let (player, mut player_rx) = Player::spawn(state.volume)
        .await
        .context("starting mpv")?;
    let (tx, mut app_rx) = mpsc::unbounded_channel::<AppMsg>();
    let covers = Arc::new(CoverLoader::new(api::cache_dir()));

    let mut app = App::new(cfg, state, api.clone(), player, covers, tx.clone());

    // MPRIS is best-effort: no session bus (plain ssh, some containers) just
    // means no media keys, never a failure to start.
    let mut mpris_rx = match mpris::Mpris::start().await {
        Ok((m, rx)) => {
            app.mpris = Some(m);
            Some(rx)
        }
        Err(e) => {
            app.notify(format!("media keys unavailable: {e}"));
            None
        }
    };
    if let Some(w) = warning {
        app.notify(w);
    }

    // `ytkew <query>` behaves like kew: start playing at once, then let the
    // radio mix keep it going.
    if !cli.query.is_empty() {
        let q = cli.query.join(" ");
        app.notify(format!("searching {q}…"));
        match api.search_songs(&q).await {
            Ok(tracks) if !tracks.is_empty() => {
                let seed = tracks[0].video_id.clone();
                app.play_all(tracks, 0);
                if app.cfg.autoplay_radio {
                    app.append_radio(&seed);
                }
            }
            Ok(_) => app.notify(format!("nothing found for {q}")),
            Err(e) => app.notify(format!("search failed: {e}")),
        }
    }

    let mut terminal = ratatui::init();
    // Measure the terminal's graphics cell size while nothing else is reading
    // input. Reported sizes are unreliable -- WezTerm answers CSI 16 t in
    // device pixels but draws sixel in logical pixels -- so the probe is what
    // actually decides how large the cover is emitted.
    app.calibrate_cells();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    );
    // btop-style pointer interaction: clicking tabs, rows and the progress
    // bar, plus wheel scrolling.
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableMouseCapture
    );
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(FRAME);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        // Pull the latest transport state before drawing so the progress bar
        // and clock never lag a frame behind.
        app.player_state = app.player.state().await;
        // draw records the regions a click can land on, so it needs &mut.
        let frame_area = match terminal.draw(|f| ui::views::draw(f, &mut app)) {
            Ok(completed) => completed.area,
            Err(e) => break Err(e.into()),
        };
        // Pixel graphics live outside ratatui's model, so the cover is written
        // after the frame, into cells the renderer deliberately skipped.
        if app.view == ui::View::Track && app.graphics_active() {
            if let Some(rect) = ui::views::cover_rect(ui::views::body_rect(frame_area), &app) {
                if let Err(e) = app.paint_graphics(rect) {
                    app.notify(format!("cover: {e}"));
                }
            }
        }
        if app.should_quit {
            break Ok(());
        }

        app.sync_mpris().await;

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(ev)) => {
                        if let Err(e) = handle_event(&mut app, ev).await {
                            app.notify(format!("{e}"));
                        }
                    }
                    Some(Err(e)) => break Err(e.into()),
                    None => break Ok(()),
                }
            }
            Some(pe) = player_rx.recv() => app.handle_player_event(pe).await,
            Some(cmd) = async {
                match mpris_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    // Without a bus this branch must never resolve, or select!
                    // would spin on it.
                    None => std::future::pending().await,
                }
            } => {
                if let Err(e) = app.handle_mpris(cmd).await {
                    app.notify(format!("{e}"));
                }
            }
            Some(msg) = app_rx.recv() => app.handle_app_msg(msg),
            _ = ticker.tick() => {}
        }
    };

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture
    );
    ratatui::restore();
    app.visual.stop();
    app.player.shutdown().await;
    // Persist runtime state only. config.toml is the user's file.
    let _ = app.runtime_state().save(&cfg_dir);
    result
}

async fn handle_event(app: &mut App, ev: Event) -> Result<()> {
    // A resize changes both the cell grid and possibly the pixel geometry, so
    // any sixel already on screen is stale.
    if let Event::Resize(_, _) = ev {
        // Cell size is a font property, not a window property, so it does not
        // need re-querying here -- only the placed image is stale.
        app.invalidate_sixel();
        return Ok(());
    }
    if let Event::Mouse(me) = ev {
        return app.handle_mouse(me).await;
    }
    let Event::Key(key) = ev else {
        return Ok(());
    };
    // Windows sends key-release events too; only act on presses.
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    // While the search box has focus, typing must reach the box rather than
    // triggering playback shortcuts.
    if app.search_editing {
        match key.code {
            KeyCode::Enter => app.submit_search(),
            KeyCode::Esc => app.search_editing = false,
            KeyCode::Backspace => {
                app.search_input.pop();
                app.refresh_suggestions();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_input.push(c);
                app.refresh_suggestions();
            }
            _ => {}
        }
        return Ok(());
    }

    // Re-entering the search view with '/' or typing in it resumes editing.
    if app.view == ui::View::Search && matches!(key.code, KeyCode::Char('i')) {
        app.search_editing = true;
        return Ok(());
    }

    if let Some(action) = app.keymap.resolve(key.code, key.modifiers) {
        app.handle_action(action).await?;
    }
    Ok(())
}

/// Guided credential setup. Kept deliberately chatty -- this is the one part
/// of the app the user only touches when something is confusing.
async fn run_auth(method: &str, cfg_dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(cfg_dir)?;
    match method {
        "cookie" => {
            println!("YouTube Music cookie setup");
            println!();
            println!("  1. Open https://music.youtube.com in your browser, signed in.");
            println!("  2. Open devtools (F12) -> Network tab.");
            println!("  3. Click any request to music.youtube.com.");
            println!("  4. Under Request Headers, copy the entire 'Cookie' value.");
            println!();
            println!("Paste it here and press enter:");

            let mut cookie = String::new();
            std::io::stdin()
                .read_line(&mut cookie)
                .context("reading cookie from stdin")?;
            let cookie = cookie.trim();
            if cookie.is_empty() {
                anyhow::bail!("no cookie provided");
            }

            // Validate before saving, so a bad paste fails now and not later.
            print!("checking… ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            match ytmapi_rs::YtMusic::from_cookie(cookie).await {
                Ok(yt) => match yt.get_library_playlists().await {
                    Ok(pls) => {
                        let path = cfg_dir.join("cookie.txt");
                        write_secret(&path, cookie.as_bytes())?;
                        println!("ok — {} playlists visible", pls.len());
                        println!("saved to {}", path.display());
                    }
                    Err(e) => anyhow::bail!("cookie was rejected by the API: {e}"),
                },
                Err(e) => anyhow::bail!("cookie could not be parsed: {e}"),
            }
        }
        "oauth" => {
            println!("OAuth setup needs a Google Cloud OAuth client of type");
            println!("'TVs and Limited Input devices'.");
            println!();
            print!("Client ID: ");
            let client_id = read_line()?;
            print!("Client secret: ");
            let client_secret = read_line()?;

            let client = ytmapi_rs::Client::new().context("building http client")?;
            let (code, url) = ytmapi_rs::generate_oauth_code_and_url(&client, &client_id).await?;
            println!();
            println!("Go to {url}, finish the login, then press enter here.");
            let _ = read_line()?;

            let token =
                ytmapi_rs::generate_oauth_token(&client, code, client_id, client_secret).await?;
            let path = cfg_dir.join("oauth.json");
            write_secret(&path, serde_json::to_string_pretty(&token)?.as_bytes())?;
            println!("saved to {}", path.display());
        }
        other => anyhow::bail!("unknown auth method {other:?} (use 'cookie' or 'oauth')"),
    }
    Ok(())
}

/// Print what each library endpoint returns, so an empty library can be told
/// apart from a credential or parsing problem.
async fn run_diagnose(cfg_dir: &std::path::Path) -> Result<()> {
    // Probe first: the measurement needs a screen it can draw on without
    // scrolling, so it must happen before anything is printed.
    let cfg_probe = config::Config::load(cfg_dir);
    let ((cw, ch), src) = sixel::detect_cell_size(match cfg_probe.cell_px {
        [w, h] if w > 0 && h > 0 => Some((w, h)),
        _ => None,
    });
    let measured = if src == sixel::CellSource::Config {
        None
    } else {
        sixel::calibrate((cw, ch))
    };

    println!("config dir: {}", cfg_dir.display());
    for f in ["cookie.txt", "oauth.json", "config.toml", "state.toml"] {
        let p = cfg_dir.join(f);
        println!(
            "  {f:<12} {}",
            if p.exists() { "present" } else { "-" }
        );
    }

    println!();
    println!("terminal:");
    println!(
        "  TERM={}  TERM_PROGRAM={}",
        std::env::var("TERM").unwrap_or_default(),
        std::env::var("TERM_PROGRAM").unwrap_or_default()
    );
    println!(
        "  multiplexer:     {}",
        sixel::multiplexer().unwrap_or("none")
    );
    println!("  cell size:       {cw}x{ch} px (from {src:?})");
    println!("  sixel terminal:  {}", sixel::terminal_supports_sixel());
    println!("  cover_mode:      {:?}", cfg_probe.cover_mode);
    println!(
        "  kitty graphics:  {}",
        if kitty::terminal_supports_kitty() {
            "yes"
        } else if sixel::multiplexer() == Some("zellij") {
            "no — needs zellij 0.45.0 or newer"
        } else {
            "no"
        }
    );

    // Print the exact geometry the renderer would use, so a wrong-sized cover
    // can be diagnosed from numbers instead of eyeballing a screenshot.
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        let (ecw, ech) = measured.unwrap_or((cw, ch));
        println!("  pane:            {cols} cols x {rows} rows");
        // Mirrors ui::views::cover_rect.
        let viz_h: u16 = match cfg_probe.visualizer_mode {
            config::VisualizerMode::Off => 0,
            _ => cfg_probe.visualizer_height,
        };
        let body_h = rows.saturating_sub(2);
        let chrome = 1 + 3 + 1 + 1;
        let cover_h = body_h.saturating_sub(chrome + viz_h);
        let ratio = (ech as f32 / ecw.max(1) as f32).max(1.0);
        let cover_w = ((cover_h as f32 * ratio).round() as u16).min(cols).max(1);
        println!("  cover area:      {cover_w} cols x {cover_h} rows");
        println!(
            "  cover image:     {}x{} px  (cover area x {ecw}x{ech})",
            cover_w as u32 * ecw as u32,
            cover_h as u32 * ech as u32
        );
        println!(
            "  pane in px:      {}x{} px  -- the image must fit inside this",
            cols as u32 * ecw as u32,
            rows as u32 * ech as u32
        );
    }
    match measured {
        Some((mw, mh)) => {
            println!("  measured cell:   {mw}x{mh} px (from a sixel probe)");
            println!("  -> ytkew will use the measured size for sixel.");
            if (mw, mh) != (cw, ch) {
                println!("     (the terminal reported {cw}x{ch}; pin with cell_px = [{mw}, {mh}])");
            }
        }
        None if src != sixel::CellSource::Config => {
            println!("  measured cell:   probe got no usable response");
        }
        None => {}
    }
    let effective = measured.map(|_| true).unwrap_or(src.is_trustworthy());
    println!("  -> sixel usable: {}", effective && sixel::terminal_supports_sixel());
    if !effective {
        println!("     cell size could not be confirmed, so `auto` uses half-blocks.");
        println!("     Set cell_px = [w, h] in config.toml to force sixel safely.");
    }

    let (api, warning) = api::Api::connect(cfg_dir).await;
    println!();
    println!("authenticated: {}", api.is_authenticated());
    println!("offline:       {}", api.is_offline());
    if let Some(w) = warning {
        println!("warning:       {w}");
    }
    println!();

    macro_rules! probe {
        ($label:expr, $call:expr) => {
            match $call.await {
                Ok(v) => println!("  {:<22} {}", $label, v),
                Err(e) => println!("  {:<22} ERROR: {e}", $label),
            }
        };
    }

    println!("library:");
    match api.library_playlists().await {
        Ok(pls) => {
            println!("  {:<22} {}", "playlists", pls.len());
            for p in pls.iter().take(10) {
                println!("      - {} ({}) [{}]", p.title, p.track_count, p.id);
            }
        }
        Err(e) => println!("  {:<22} ERROR: {e}", "playlists"),
    }
    probe!("library songs", async { api.library_songs().await.map(|v| v.len()) });
    probe!("library albums", async { api.library_albums().await.map(|v| v.len()) });
    probe!("library artists", async { api.library_artists().await.map(|v| v.len()) });
    probe!("history periods", api.history_count());

    println!();
    println!("liked music (LM auto-playlist):");
    match api.liked_songs().await {
        Ok(t) => {
            println!("  {:<22} {} tracks", "liked", t.len());
            for tr in t.iter().take(10) {
                println!("      - {} — {}", tr.artist, tr.title);
            }
            if t.is_empty() {
                println!("  note: YouTube likes are separate from YouTube Music likes.");
                println!("        Enable 'liked music from YouTube' in YouTube Music settings");
                println!("        to surface them here.");
            }
        }
        Err(e) => println!("  {:<22} ERROR: {e}", "liked"),
    }
    println!();
    Ok(())
}

/// Write a credential with owner-only permissions. A session cookie grants
/// full account access, so it must not be group- or world-readable.
fn write_secret(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting permissions on {}", path.display()))?;
    }
    Ok(())
}

fn read_line() -> Result<String> {
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}
