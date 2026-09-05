# ytkew

A terminal YouTube Music player. Takes its interface, keybindings and general
philosophy from [kew](https://github.com/ravachol/kew) — cover art, spectrum
visualizer, album-derived colours, no distractions — but plays from your
YouTube Music account instead of local files.

```
ytkew radiohead creep     # search and start playing, kew-style
ytkew                     # just open the TUI
```

## How it works

Four moving parts, deliberately separated:

| Layer | What does it | Why |
|---|---|---|
| [`ytmapi-rs`](https://github.com/nick42d/youtui) | YouTube Music's internal API | Someone else maintains the protocol layer, which is the part YouTube breaks |
| **mpv** over JSON IPC | streaming, decoding, seeking, gapless | Never reimplement a player; a slow redraw can't stutter audio |
| **ratatui** | interface | |
| **PipeWire** sink monitor + FFT | spectrum visualizer | mpv exposes no sample data, so we tap the output like `cava` does |
| **kitty graphics** / hand-rolled **sixel** | full-resolution album art | kitty sizes placements in cells, dodging cell-pixel detection entirely; sixel is the fallback, hand-rolled because chafa sizes from a TTY query it can't make through a pipe |

Playback keeps two tracks in mpv's playlist at all times — the current one and
the next. Every YouTube track needs a yt-dlp round trip to resolve (~1-7s
cold), so without prefetching, every transition would stall. With it, only the
very first track waits, and skipping is instant because the next stream is
already buffered.

## Album art

`cover_mode = "auto"` picks the best renderer available, in this order:

1. **Kitty graphics protocol.** Placements are sized in *cells* (`c=`/`r=`),
   so the terminal does the scaling and no cell pixel size is involved. This
   is the only protocol immune to the sizing problems below, and it is
   preferred wherever it works: WezTerm, kitty, ghostty, Konsole, and zellij
   from **v0.45.0** onward.
2. **Sixel.** Needs an accurate cell pixel size, which is the fragile part.
   Only used when that size is pinned or successfully measured.
3. **Truecolor half-blocks**, drawn into ratatui's own buffer. Always
   correctly sized, works anywhere, just softer.

`b` shows and hides the art. *Which* renderer is used is a setting, not
something to cycle past on the way to turning it off -- set `cover_mode` in
the config, or pick it from the `esc` menu, which shows the current choice.
`[` / `]` resize sixel art by eye if its scale is off.

Support is confirmed by the protocol'"'"'s own capability query rather than
guessed from `TERM_PROGRAM`: a wrong guess would dump kilobytes of base64
across the screen as literal text.

### Why kitty is preferred

Sixel takes pixel dimensions, so it needs the terminal's cell size in pixels.
Nothing reports that reliably:

- Under zellij, the tty window size gives the *outer window's* pixels with the
  *pane's* cell count, so dividing them is nonsense.
- WezTerm answers `CSI 16 t`, but the number can be a whole display-scale
  factor away from what it uses for graphics.
- Measuring it by drawing a probe and reading the cursor back works, but only
  to within the row quantisation, and sixel leaves the cursor on the row
  *containing* the image bottom -- a floored count that over-estimates the
  cell if you divide naively.
- zellij additionally has an open regression
  ([#3372](https://github.com/zellij-org/zellij/issues/3372)) that renders
  sixel at double height regardless.

Kitty sidesteps all of it by letting you say "18 rows by 36 columns".

Sixel needs the terminal's cell size in pixels to scale the image correctly.
Get it wrong and the art overflows its reserved area -- inside a multiplexer,
that means painting over neighbouring panes. So ytkew resolves it in order of
reliability:

1. `cell_px` from your config, if set.
2. **A measurement.** ytkew draws a sixel strip of known pixel height and
   reads back how many rows the cursor advanced. This is the only source that
   reflects what the terminal actually does, and it is the default.
3. A `CSI 16 t` query. Useful for the aspect ratio, but not the absolute
   scale: WezTerm on a HiDPI display answers in *device* pixels while laying
   sixel out in *logical* pixels, so its numbers are 2x too large.
4. The tty window size, if it is a plausible font metric.
5. A conventional 8x16 guess.

**`auto` only draws sixel for sources 1 and 2.** Under zellij and tmux, the
window size is reported for the outer window while the cell count is the
pane's, so dividing them yields nonsense -- ytkew rejects that and falls back
to half-blocks rather than corrupting your other panes.

If `ytkew --diagnose` says the cell size could not be confirmed, set it
explicitly to get sharp art back:

```toml
cell_px = [16, 32]   # whatever your terminal actually uses
```

## Requirements

- `mpv` and `yt-dlp` on PATH
- A truecolor terminal (cover art uses half-block rendering)
- PipeWire, for the visualizer only — everything else works without it

## Install

```sh
cargo build --release
install -Dm755 target/release/ytkew ~/.local/bin/ytkew
```

## Diagnosing

```sh
ytkew --diagnose
```

Reports which credential files are present and what each library endpoint
returns, which separates "my library is empty" from "auth is broken".

## Signing in

**Search, radio, playlists by ID and lyrics need no credentials at all.** Only
your own library (playlists, liked songs, liking tracks) does.

```sh
ytkew --auth cookie      # paste your browser Cookie header; validated before saving
ytkew --auth oauth       # needs a Google Cloud "TVs and Limited Input devices" client
```

Cookie auth is easier and doesn't expire on a timer. Both land in
`~/.config/ytkew/`.

## Keys

kew's bindings, unchanged where they made sense.

| | |
|---|---|
| `space` `p` | play / pause |
| `h` `l` or `←` `→` | previous / next track |
| `j` `k` or `↑` `↓` | move selection |
| `a` `d` | seek back / forward |
| `+` `-` | volume |
| `s` | shuffle |
| `r` | repeat (off → all → one) |
| `v` | cycle visualizer (bars → braille → off) |
| `b` | show / hide album art |
| `.` | like current track |
| `R` | start radio from current track |
| `enter` | add to queue (plays if idle) |
| `alt+enter` `ctrl+g` | add and jump to it |
| `[` `]` | resize sixel art (only needed if its scale is off) |
| `f` `g` | move track up / down in queue |
| `del` / `bksp` | remove one / clear the whole queue |
| `esc` | menu (play all, shuffle, repeat, clear queue, help, quit) |
| `P` | play everything in the current view |
| `1`–`4` | jump to a tab, as numbered in the strip |
| `F2`–`F6` | queue, library, track, search, help |
| `tab` | cycle views |
| `m` | lyrics |
| `q` `esc` | quit |

`/` also opens search. In the search box, `enter` searches, `esc` leaves
editing, and autocomplete appears as you type.

## Configuration

Two files in `~/.config/ytkew/`:

- **`config.toml`** is yours. ytkew only ever *reads* it, so comments and
  hand-edits are safe. A commented default is written on first run if absent.
- **`state.toml`** is the app's: volume, and shuffle/repeat when
  `save_repeat_shuffle = true`. Rewritten on exit.

```toml
visualizer_height = 6
visualizer_bar_width = 2
visualizer_mode = "bars"      # bars | braille | off

cover_mode = "auto"           # auto | sixel | blocks | off
cell_px = [0, 0]              # cell size in px; [0,0] autodetects
cover_enabled = true
color_from_cover = true       # derive the theme from album art
accent_color = 6              # ANSI index, used when color_from_cover = false

initial_volume = 100.0        # only applies before state.toml exists
volume_step = 5.0
seek_step = 5.0

autoplay_radio = true         # append a radio mix behind a played search hit
save_repeat_shuffle = false   # remember shuffle/repeat across restarts
hide_help = false
```

## Known limits

- Unofficial API: YouTube can change it at any time. Bump `ytmapi-rs` first.
- The visualizer captures the whole audio sink, not just ytkew, so other
  applications' sound shows up in the bars. This is how `cava` behaves too.
- Non-Premium accounts may get lower-bitrate streams.
- Library browsing is playlists and liked songs; there's no artist/album tree
  yet.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

## Credits

The interface follows [kew](https://github.com/ravachol/kew) by ravachol --
its layout, keybindings, spectrum visualizer and album-derived theming are the
model this was built against. No kew code is used here; ytkew is written from
scratch in Rust against a streaming backend rather than local files.

The panel styling and overlays take after
[binsider](https://github.com/orhun/binsider), and the mouse handling and menu
after [btop](https://github.com/aristocratos/btop).

YouTube Music access is via [ytmapi-rs](https://github.com/nick42d/youtui).
This project is not affiliated with, endorsed by, or connected to YouTube or
Google.
