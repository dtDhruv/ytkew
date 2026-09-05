//! Command line surface: argument parsing and the one-shot subcommands.

use crate::{api, art, config};
use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "ytkew",
    about = "A terminal YouTube Music player, in the spirit of kew",
    version
)]
pub struct Cli {
    /// Search terms -- plays the best match immediately, like `kew nirvana`.
    #[arg(trailing_var_arg = true)]
    pub query: Vec<String>,

    /// Set up credentials: `browser` lifts an existing Firefox login,
    /// `cookie` takes a pasted header, `oauth` runs a device flow.
    #[arg(long, value_name = "METHOD")]
    pub auth: Option<String>,

    /// Report what the API can see with the current credentials.
    #[arg(long)]
    pub diagnose: bool,
}

/// Guided credential setup. Kept deliberately chatty -- this is the one part
/// of the app the user only touches when something is confusing.
pub async fn run_auth(method: &str, cfg_dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(cfg_dir)?;
    match method {
        "browser" => {
            println!("Looking for a signed-in YouTube Music session in Firefox…");
            let found = crate::browser::find_cookies()?;
            println!("  profile: {}", found.profile.display());
            println!("  cookies: {}", found.names.join(", "));
            println!();
            // Same validation as a pasted header: a cookie that reads fine
            // but the API rejects is worse than no cookie at all.
            print!("checking… ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            match ytmapi_rs::YtMusic::from_cookie(&found.header).await {
                Ok(yt) => match yt.get_library_playlists().await {
                    Ok(pls) => {
                        let path = cfg_dir.join("cookie.txt");
                        write_secret(&path, found.header.as_bytes())?;
                        println!("ok — {} playlists visible", pls.len());
                        println!("saved to {}", path.display());
                    }
                    Err(e) => anyhow::bail!("the browser's cookies were rejected by the API: {e}"),
                },
                Err(e) => anyhow::bail!("the browser's cookies could not be parsed: {e}"),
            }
        }
        "cookie" => {
            println!("YouTube Music cookie setup");
            println!();
            println!("If you use Firefox, `ytkew --auth browser` does this for you.");
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
        other => {
            anyhow::bail!("unknown auth method {other:?} (use 'browser', 'cookie' or 'oauth')")
        }
    }
    Ok(())
}

/// Print what each library endpoint returns, so an empty library can be told
/// apart from a credential or parsing problem.
pub async fn run_diagnose(cfg_dir: &std::path::Path) -> Result<()> {
    // Probe first: the measurement needs a screen it can draw on without
    // scrolling, so it must happen before anything is printed.
    let cfg_probe = config::Config::load(cfg_dir);
    let ((cw, ch), src) = art::terminal::detect_cell_size(match cfg_probe.cell_px {
        [w, h] if w > 0 && h > 0 => Some((w, h)),
        _ => None,
    });
    let measured = if src == art::terminal::CellSource::Config {
        None
    } else {
        art::terminal::calibrate((cw, ch))
    };

    println!("config dir: {}", cfg_dir.display());
    for f in ["cookie.txt", "oauth.json", "config.toml", "state.toml"] {
        let p = cfg_dir.join(f);
        println!("  {f:<12} {}", if p.exists() { "present" } else { "-" });
    }

    println!();
    println!("playback:");
    // The two external programs ytkew cannot work without. Reported here
    // because a missing extractor otherwise presents as a player that runs
    // perfectly and never makes a sound.
    let cfg = crate::config::Config::load(cfg_dir);
    match crate::player::find_extractor(&cfg.ytdlp_path) {
        Ok(path) => println!("  yt-dlp       {}", path.display()),
        Err(_) => println!("  yt-dlp       MISSING -- nothing will play"),
    }
    match std::process::Command::new("mpv")
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) => {
            let v = String::from_utf8_lossy(&out.stdout);
            println!("  mpv          {}", v.lines().next().unwrap_or("present"));
        }
        Err(_) => println!("  mpv          MISSING -- nothing will play"),
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
        art::terminal::multiplexer().unwrap_or("none")
    );
    println!("  cell size:       {cw}x{ch} px (from {src:?})");
    println!(
        "  sixel terminal:  {}",
        art::terminal::terminal_supports_sixel()
    );
    println!("  cover_mode:      {:?}", cfg_probe.cover_mode);
    println!(
        "  kitty graphics:  {}",
        if art::kitty::terminal_supports_kitty() {
            "yes"
        } else if art::terminal::multiplexer() == Some("zellij") {
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
        None if src != art::terminal::CellSource::Config => {
            println!("  measured cell:   probe got no usable response");
        }
        None => {}
    }
    let effective = measured.map(|_| true).unwrap_or(src.is_trustworthy());
    println!(
        "  -> sixel usable: {}",
        effective && art::terminal::terminal_supports_sixel()
    );
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
    probe!("library songs", async {
        api.library_songs().await.map(|v| v.len())
    });
    probe!("library albums", async {
        api.library_albums().await.map(|v| v.len())
    });
    probe!("library artists", async {
        api.library_artists().await.map(|v| v.len())
    });
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
