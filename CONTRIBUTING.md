# Contributing

Thanks for taking a look. Bug reports, patches and terminal-compatibility
notes are all welcome.

## Getting set up

```sh
cargo build
cargo test
```

Running it needs `mpv` and `yt-dlp` on `PATH`. Search works with no
credentials at all, so you can develop most of the app without signing in;
only the library needs `ytkew --auth cookie`.

`ytkew --diagnose` prints what the terminal reports about itself, which
credentials are present, and what each library endpoint returns. Start there
when something looks wrong.

## Before opening a pull request

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

The tree is kept at zero warnings and zero clippy findings. CI runs the same
three commands.

## Where things live

| Path | What it does |
|---|---|
| `src/run.rs` | the event loop -- start here |
| `src/app/` | all interface state; `input` maps keys and mouse, `library` the tree, `menu` the overlay, `graphics` album art, `media` D-Bus, `search` API calls |
| `src/ui/` | rendering, one module per pane, plus `layout` for shared geometry |
| `src/art/` | cover rendering: `kitty`, `sixel`, `terminal` probing, `quantize` colours |
| `src/api.rs`, `src/model.rs` | YouTube Music, normalised into one `Track` type |
| `src/player.rs`, `src/queue.rs` | mpv over JSON IPC, and the play order |
| `src/config/` | `Config` is the user's file and is never written; `State` is what the app remembers |

`cargo doc --open` renders the architecture notes in `src/lib.rs`.

## Conventions

**Comments explain why, not what.** The code already says what it does. A
comment earns its place when it records a decision, a constraint, or a
non-obvious reason -- for example why kitty is preferred over sixel, or why
`IdentitiesOnly` matters. Delete anything that restates the line below it.

**Tests describe behaviour.** Name them as sentences (`the_playhead_marker_tracks_position`)
and assert what a user would notice. Rendering is testable without a terminal
via ratatui's `TestBackend`; several widget tests do exactly that.

**Terminal quirks belong in comments.** A surprising amount of this code
exists because terminals disagree with each other. When you work around one,
say which terminal and why, and link the upstream issue if there is one.
