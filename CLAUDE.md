# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

ytkew is a terminal YouTube Music player. It is an **application that exposes a
library for testing**, not a library anyone depends on — which is why errors are
`anyhow::Result` with `.context()` throughout and there is no typed error enum.

## Commands

```sh
cargo build                       # debug
cargo test                        # all tests
cargo test the_test_name          # one test, by substring of its name
cargo test player::               # one module's tests
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
make check                        # fmt --check + clippy + test, what CI runs
make install                      # binary + desktop entry + icon into ~/.local
```

**Both clippy invocations must pass.** `browser-cookies` is a default feature
gating all of `src/browser.rs`; without the second run it rots unnoticed.

Running it needs `mpv` and `yt-dlp` on `PATH`. **Search works with no
credentials**, so most of the app can be developed signed out; only the library
needs `ytkew --auth browser` (Firefox) or `--auth cookie`.

Always develop against a throwaway config — ytkew rewrites `state.toml` on exit,
so a test run will happily save volume 0 over real settings:

```sh
XDG_CONFIG_HOME=/tmp/ytkew-dev cargo run -- radiohead creep
```

`ytkew --diagnose` reports which of mpv/yt-dlp it found and their versions, what
the terminal claims about itself, and what each library endpoint returns. Start
there when something looks wrong.

### Non-Rust checks CI also runs

```sh
python3 .github/scripts/gen_assets.py      # regenerate derived assets
python3 .github/scripts/check_secrets.py   # credential hygiene, tree + history
cd docs && npm ci && npm run build         # the Astro Starlight site
python3 .github/scripts/check_links.py docs/dist --base /ytkew
```

## Architecture

Four separated concerns, and the separation is deliberate:

| Layer | Does | Why |
|---|---|---|
| `ytmapi-rs` | YouTube Music's internal API | Someone else maintains the protocol layer, which is what YouTube breaks |
| **mpv**, over JSON IPC on a Unix socket | streaming, decoding, seeking, gapless | Never reimplement a player; a slow redraw cannot stutter audio |
| **ratatui** | the interface | |
| **PipeWire** (`pw-cat`) + rustfft | the spectrum | mpv exposes no sample data, so the sink monitor is tapped like `cava` |

`src/run.rs` holds the event loop — **start reading there.** It selects over
crossterm events, mpv events, `AppMsg`s from background tasks, and a frame
ticker.

`src/app.rs` plus `src/app/` is all interface state. Every long operation is
spawned onto tokio and replies with an `AppMsg` tagged by what it answers (a
library path, a search filter, a video id), so a slow response can never land on
the wrong pane after the user has navigated away.

`src/ui/` renders, one module per pane, and never mutates app state except to
record click regions.

### Invariants that are easy to break

**All track-view geometry goes through `src/ui/layout.rs`.** The renderer
reserves cells for album art and a *separate pass after the frame* writes pixels
over them. If those two disagree by one row, artwork lands on text or the
previous pane's text shows through. Both callers use the same functions for
exactly this reason — this has broken twice.

**Cells reserved for graphics are skipped by ratatui's diff**, so nothing is
emitted for them however stale the terminal actually is. Anything needing those
cells cleared must write to the terminal directly — see `clear_cover_art` and
`paint_graphics` in `src/app/graphics.rs`.

**A pixel image is not part of the cell grid**, so no overlay drawn into
ratatui's buffer can cover it. The menu takes the image off screen *before* the
frame rather than drawing over it.

**`Config` is the user's file and is never written; `State` is the app's and is
rewritten on exit.** Keeping them apart is what makes hand-edits and comments in
`config.toml` safe.

**`Queue` keeps `tracks` (what you see) and `order` (what plays) separate**, so
toggling shuffle never scrambles the visible list. `queue_origin` records what
filled the queue, which is what makes YouTube Music's context rules work —
playing a track from the same playlist moves within the queue, from a different
one replaces it, and a one-off from search inserts next.

**The API layer dispatches through macros** (`any_auth!`, `logged_in!`) because
`ytmapi-rs`'s `AuthToken` is not object-safe — it returns `impl IntoIterator` and
has a generic associated function, so `Box<dyn AuthToken>` is impossible. The
alternative was the same four-arm match in every method.

## Conventions

Formatting and naming are not choices: rustfmt owns layout, RFC 430 naming is
compiler-enforced, and lint levels live in `[lints]` in `Cargo.toml` so local and
CI clippy agree. `unwrap` and `expect` are **denied outside tests** — a music
player has no business panicking.

- **Reach for a crate before writing it yourself.** Hand-rolling has to earn
  itself and the comment above it must say what it earned. `src/art/sixel.rs`
  qualifies (chafa sizes its output from a TTY query it cannot make through a
  pipe); a hand-rolled PATH search and calendar arithmetic did not, and were
  replaced by `which` and `time`.
- **Comments explain why, not what.** Delete anything restating the line below.
- **Terminal quirks belong in comments** — say which terminal, and link the
  upstream issue. A surprising amount of `src/art/` exists because terminals
  disagree with each other.
- **Tests are named as sentences** describing behaviour
  (`a_saved_zero_volume_does_not_come_back_silent`).
- **Modules use the 2018 layout**: `src/app.rs` beside `src/app/`, no `mod.rs`.
- **Commit subjects are imperative and impersonal**, body explains why. No first
  person — a reader of `git log` should not wonder who "I" is.

## Things with a single source of truth

- **The icon** is `assets/ytkew.svg`. The binary embeds that path with
  `include_str!`, the Makefile installs it, and the site's two copies are
  generated. Run `.github/scripts/gen_assets.py` after changing it; CI fails on stale copies.
  Keep the *file name* stable — `Icon=ytkew` resolves by name and users already
  have it in their icon theme.
- **The wordmark** is generated from the same ANSI Shadow grid
  `src/ui/banner.rs` draws. Pasting block characters into HTML does not work:
  browsers do not promise to tile `█` and `╗╝║═`, and the letterforms come apart.
- **The MSRV** in `Cargo.toml` tracks the floor the dependency tree already
  imposes, not a number anyone picked.

## Docs

`docs/` is an Astro Starlight site published to GitHub Pages at
`dtdhruv.github.io/ytkew`, served under `/ytkew/`, so absolute internal links
need that prefix. Pages are Markdown under `docs/src/content/docs/`; the sidebar
is declared in `docs/astro.config.mjs`, so a new page needs an entry there.

The README is deliberately a landing page — install steps, keys and
configuration live on the site, not in it.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contributor-facing version of all
of this.
