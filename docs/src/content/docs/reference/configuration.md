---
title: Configuration
description: Every option, what it does, and what it defaults to.
---

Two files in `~/.config/ytkew/`:

- **`config.toml`** is yours. ytkew only ever *reads* it, so comments and
  hand-edits are safe. A commented default is written on first run if absent.
- **`state.toml`** is the app's: volume, the chosen theme, and shuffle and
  repeat when you ask for them to be remembered. Rewritten on exit.

Unknown keys are ignored rather than fatal, so a config from a newer version
still starts.

## Options

| Key | Default | Means |
|---|---|---|
| `theme` | `"cover"` | A built-in, one of your own, or `cover` for the album palette. |
| `keys` | `"kew"` | `kew` or `vim`. |
| `side_pane` | `"queue"` | `off`, `queue` or `library`. |
| `library_layout` | `"columns"` | `columns` or `tree`. |
| `cover_mode` | `"auto"` | `auto`, `kitty`, `sixel`, `blocks` or `off`. |
| `cover_enabled` | `true` | Draw artwork at all. |
| `cell_px` | `[0, 0]` | Cell size in pixels for sixel. `[0,0]` autodetects. |
| `visualizer_mode` | `"bars"` | `bars`, `braille` or `off`. |
| `visualizer_height` | `6` | Rows the spectrum occupies. |
| `visualizer_bar_width` | `2` | Columns per bar. |
| `initial_volume` | `100.0` | Only applies before `state.toml` exists. |
| `volume_max` | `100.0` | Raise to at most 130 to allow boosting. See below. |
| `volume_step` | `5.0` | Percent per keypress. |
| `seek_step` | `5.0` | Seconds per keypress. |
| `autoplay_radio` | `true` | Append a radio mix behind a played search hit. |
| `save_repeat_shuffle` | `false` | Remember them across restarts. |
| `accent_color` | `6` | ANSI index, used only as a last resort. |
| `theme_colors` | — | Three hex strings, with `theme = "custom"`. |
| `hide_help` | `false` | Hide the hint line. |

## On volume above 100%

mpv can go to 130, but past 100 it is plain digital gain with nothing to catch
the peaks, so anything mastered near full scale clips and turns fuzzy. ytkew
caps at 100 by default; raise `volume_max` if you want the headroom for quiet
recordings.

A saved volume of zero is never restored either — it comes back at
`initial_volume` instead. A player that looks like it is working while making
no sound gives you nothing to go on.

## Example

```toml
theme = "gruvbox"
keys = "vim"
side_pane = "library"
library_layout = "columns"

cover_mode = "kitty"
visualizer_mode = "braille"
visualizer_height = 8

volume_max = 130.0
autoplay_radio = false
save_repeat_shuffle = true
```
