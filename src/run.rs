//! The event loop.
//!
//! One task owns the terminal and the [`App`]; everything slower than a frame
//! -- API calls, cover downloads -- runs elsewhere and reports back over
//! channels, so a redraw never waits on the network.

use crate::app::{App, AppMsg};
use crate::art::CoverLoader;
use crate::cli::{run_auth, run_diagnose, Cli};
use crate::player::Player;
use crate::{api, config, mpris, ui};
use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Redraw cadence. The visualizer needs a steady tick; everything else is
/// event-driven.
const FRAME: Duration = Duration::from_millis(33);

/// Start the player and run until the user quits.
pub async fn run(cli: Cli) -> Result<()> {
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

    let (player, mut player_rx) = Player::spawn(state.volume).await.context("starting mpv")?;
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
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(FRAME);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        // Pull the latest transport state before drawing so the progress bar
        // and clock never lag a frame behind.
        app.player_state = app.player.state().await;
        // Take the cover off screen before drawing anything that has to appear
        // over it. A pixel image is not part of the cell grid, so this has to
        // happen ahead of the frame -- blanking afterwards would erase what
        // was just drawn.
        if !(app.view == ui::View::Track && app.graphics_active()) {
            app.clear_cover_art();
        }
        // draw records the regions a click can land on, so it needs &mut.
        let frame_area = match terminal.draw(|f| ui::views::draw(f, &mut app)) {
            Ok(completed) => completed.area,
            Err(e) => break Err(e.into()),
        };
        // Pixel graphics live outside ratatui's model, so the cover is written
        // after the frame, into cells the renderer deliberately skipped.
        if app.view == ui::View::Track && app.graphics_active() {
            if let Some(rect) =
                ui::layout::cover_rect(ui::layout::body_rect(frame_area, &app), &app)
            {
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

    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
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
        app.clear_cover_art();
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
