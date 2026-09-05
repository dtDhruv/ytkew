---
title: Architecture
description: How the pieces fit, for anyone reading or changing the code.
---

Four moving parts, deliberately separated.

| Layer | Does | Why |
|---|---|---|
| [ytmapi-rs](https://github.com/nick42d/youtui) | YouTube Music's internal API | Someone else maintains the protocol layer, which is the part YouTube breaks. |
| **mpv**, over JSON IPC | streaming, decoding, seeking, gapless | Never reimplement a player; a slow redraw cannot stutter audio. |
| **ratatui** | the interface | |
| **PipeWire** + FFT | the spectrum | mpv exposes no sample data, so the output is tapped the way cava does it. |

## Where things live

| Path | What it does |
|---|---|
| `src/run.rs` | the event loop — start here |
| `src/app/` | interface state: `input`, `library`, `menu`, `graphics`, `media`, `search` |
| `src/ui/` | rendering, one module per pane, plus `layout` for shared geometry |
| `src/art/` | cover rendering: `kitty`, `sixel`, `terminal` probing, `quantize` |
| `src/api.rs`, `src/model.rs` | YouTube Music, normalised into one `Track` |
| `src/player.rs`, `src/queue.rs` | mpv over JSON IPC, and the play order |
| `src/config/` | `Config` is the user's file; `State` is what the app remembers |
| `src/browser.rs` | reading an existing login out of the Firefox cookie store |

`cargo doc --open` renders the architecture notes in `src/lib.rs`.

## Two invariants worth knowing

**Layout goes through `src/ui/layout.rs`, always.** The renderer reserves cells
for album art and a separate pass writes pixels over them. If those two
disagree by even one row, the artwork lands on top of text or leaves the
previous pane's text showing through.

**Cells reserved for graphics are skipped by ratatui's diff**, so nothing is
emitted for them no matter what the terminal is actually showing. Anything
that needs those cells cleared has to write to the terminal directly.

## Contributing

See [CONTRIBUTING.md](https://github.com/dtDhruv/ytkew/blob/main/CONTRIBUTING.md).
Bug reports, patches, themes and terminal-compatibility notes are all welcome.
