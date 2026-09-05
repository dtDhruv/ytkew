<div align="center">

<img src="assets/ytkew.svg" width="96" alt="">

```
██╗   ██╗████████╗██╗  ██╗███████╗██╗    ██╗
╚██╗ ██╔╝╚══██╔══╝██║ ██╔╝██╔════╝██║    ██║
 ╚████╔╝    ██║   █████╔╝ █████╗  ██║ █╗ ██║
  ╚██╔╝     ██║   ██╔═██╗ ██╔══╝  ██║███╗██║
   ██║      ██║   ██║  ██╗███████╗╚███╔███╔╝
   ╚═╝      ╚═╝   ╚═╝  ╚═╝╚══════╝ ╚══╝╚══╝
```

**A terminal YouTube Music player, in the spirit of [kew](https://github.com/ravachol/kew).**

Cover art, a spectrum visualizer, album-derived colours, no distractions —
playing from your own YouTube Music account.

[Documentation](https://dtdhruv.github.io/ytkew/) ·
[Install](#install) ·
[Keys](#keys) ·
[Configuration](https://dtdhruv.github.io/ytkew/reference/configuration/) ·
[Contributing](CONTRIBUTING.md)

[![ci](https://github.com/dtDhruv/ytkew/actions/workflows/ci.yml/badge.svg)](https://github.com/dtDhruv/ytkew/actions/workflows/ci.yml)
[![security](https://github.com/dtDhruv/ytkew/actions/workflows/security.yml/badge.svg)](https://github.com/dtDhruv/ytkew/actions/workflows/security.yml)
[![licence: GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-e62525)](LICENSE)

</div>

```
 ytkew   1 queue    2 library    3 track    4 search               not signed in  1/20  vol ▪▪▪▫▫ 60% 
──────────────────────────────┘           └───────────────────────────────────────────────────────────
╭| now playing |─────────────────────────╮ ╭| up next |──────────────────────────────────────────────╮
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Creep (Acoustic)               4:19↑│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Karma Police                   4:22█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Let Down                       5:00█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          No Surprises                   3:49█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Kelly Clarkson     Creep (Live)                   4:09█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Fake Plastic Trees             4:51█│
│                                        │ │  Radiohead          Everything In Its Right Place  4:12█│
│ Creep                                  │ │  Scala & Kolacny B… Creep                          4:54█│
│ Radiohead                              │ │  Radiohead          Jigsaw Falling Into Place      4:09║│
│ Creep                                  │ │  Scott Bradlee's P… Creep (feat. Haley Reinhart)   4:44║│
│                                        │ │  Radiohead          Exit Music (For A Film)        4:28║│
│                                        │ │  Radiohead          Street Spirit (Fade Out)       4:14║│
│    ▄▅▅▇▇▁                              │ │  Radiohead          Airbag                         4:48║│
│ ▆▆▆██████▇▇                            │ │  Radiohead          You And Whose Army?            3:11║│
│ ███████████▇▅                          │ │  Radiohead          All I Need                     3:49║│
│ ██████████████▅▃▄▃▃▁▂▅▁▃▆▅▁▂▁▂▁▂▁▂▂▁▂▂ │ │  Radiohead          No Surprises                   3:50║│
│ 0:13 ━━●━━━━━━━━━━━━━━━━━━━━━━━━━ 3:58 │ │  Stone Temple Pilo… Creep                          5:33↓│
╰────────────────────────────────| 1/20 |╯ ╰─────────────────────────────────────────────────────────╯
 space play · h/l skip · a/d seek · +/- vol · s shuffle · r repeat · m lyrics · esc menu   F6 help    
```

## Why

Every YouTube Music client is either a browser tab or an Electron app. ytkew
is neither: it is a terminal program that hands playback to **mpv**, so a slow
redraw can never stutter the audio, and it looks and behaves like kew, which
got the interface right.

## Features

- **Full-resolution cover art** — kitty graphics where the terminal supports
  it, sixel where it does not, truecolor half-blocks everywhere else
- **Gapless playback** — the next track is resolved and buffered while the
  current one plays, so transitions never stall on a yt-dlp round trip
- **Spectrum visualizer** — a real FFT over the PipeWire sink monitor
- **Colours from the artwork**, or ten built-in themes, or write your own
- **vim keys** — an optional preset with `gg`, `G`, `ctrl+d`, `dd`
- **A library that browses like a file manager** — each level in its own column
- **Search across songs, videos, albums, artists and playlists**
- **A queue that behaves like YouTube Music's** — playing a one-off track
  mid-playlist slots it in next rather than throwing the playlist away
- **MPRIS** — media keys, and a now-playing panel entry with artwork
- **Mouse support** — click tabs and rows, drag the progress bar, scroll lists

## Requirements

`mpv` and `yt-dlp` on `PATH` — ytkew refuses to start without them — plus a
truecolor terminal. PipeWire is needed for the visualizer only. A YouTube
Music account is needed only for your own library; search, radio and lyrics
work signed out.

mpv does the playing; yt-dlp turns a YouTube video into a playable stream.
[Why both are needed →](https://dtdhruv.github.io/ytkew/start/install/#why-yt-dlp)

## Install

```sh
# Dependencies — apt, dnf, pacman, zypper or brew
sudo apt install mpv yt-dlp

git clone https://github.com/dtDhruv/ytkew
cd ytkew
make install
```

> [!TIP]
> **A stale `yt-dlp` is the most common reason a working ytkew stops playing.**
> YouTube changes how streams are signed and yt-dlp is what keeps up, so keep
> it current with `yt-dlp -U`. After three failures in a row ytkew checks the
> version and tells you how old it is; `ytkew --diagnose` reports it any time.

[Per-platform instructions →](https://dtdhruv.github.io/ytkew/start/install/)

That puts the binary in `~/.local/bin` and the desktop entry and icon under
`~/.local/share`. `PREFIX=/usr/local sudo make install` for a system-wide
install; `make uninstall` removes all three.

> [!NOTE]
> `cargo install` places only the binary, so ytkew will show a generic icon in
> your launcher and now-playing panel. `make install` is what puts the real
> one in place.

## Usage

```sh
ytkew                      # open the interface
ytkew radiohead creep      # search and start playing, kew-style
ytkew --auth browser       # set up credentials
ytkew --diagnose           # report what the terminal and the API can see
```

## Signing in

**Search, radio, playlists by ID and lyrics need no credentials at all.** Only
your own library does.

```sh
ytkew --auth browser     # lifts an existing Firefox login, nothing to paste
ytkew --auth cookie      # paste a Cookie header from any browser
ytkew --auth oauth       # Google Cloud "TVs and Limited Input devices" client
```

`--auth browser` reads the YouTube cookies out of your Firefox profile, so
signing in at music.youtube.com is all the setup there is. Chromium is not
supported — it encrypts cookie values against the desktop keyring.

> [!WARNING]
> A YouTube Music cookie header authenticates as you. ytkew stores it in
> `~/.config/ytkew/cookie.txt` with owner-only permissions and never sends it
> anywhere but Google. If one leaks, sign out of Google everywhere to revoke it.

## Keys

| | |
|---|---|
| `space` `p` | play / pause |
| `h` `l` · `←` `→` | previous / next track |
| `j` `k` · `↑` `↓` | move the selection |
| `a` `d` | seek back / forward |
| `+` `-` | volume |
| `enter` | play the selection |
| `A` | add to the queue without interrupting |
| `P` | play everything in the current view |
| `s` `r` | shuffle / repeat |
| `v` `b` | cycle the visualizer / show the cover |
| `.` `R` | like the track / start radio from it |
| `del` `bksp` | remove one / clear the queue |
| `m` `/` | lyrics / search |
| `esc` `q` | menu / quit |

Set `keys = "vim"` for `gg`, `G`, `ctrl+d`/`ctrl+u`, `x`, `dd`, `J`/`K` and
`H`/`L`. The hint line at the bottom is generated from whichever preset is
active, so what it shows is always the truth.

**[Full keybinding reference →](https://dtdhruv.github.io/ytkew/guide/keys/)**

## Configuration

`~/.config/ytkew/config.toml`, written with comments on first run and only
ever read — hand-edits are safe. Runtime state lives separately in
`state.toml`.

```toml
theme = "cover"               # a built-in, one of your own, or the album palette
keys = "kew"                  # kew | vim
side_pane = "queue"           # off | queue | library
library_layout = "columns"    # columns | tree
cover_mode = "auto"           # auto | kitty | sixel | blocks | off
```

**[Full configuration reference →](https://dtdhruv.github.io/ytkew/reference/configuration/)**

## Documentation

**[dtdhruv.github.io/ytkew](https://dtdhruv.github.io/ytkew/)** — install,
signing in, every key, themes, the album-art story, the full configuration
table and troubleshooting, with search.

## Contributing

Bug reports, patches and terminal-compatibility notes are all welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, the layout of
the code, and the conventions the tree follows.

## Licence

[GPL-3.0-or-later](LICENSE).

ytkew is not affiliated with, endorsed by, or connected to YouTube or Google.
It talks to an unofficial API that can change at any time.

## Credits

- [kew](https://github.com/ravachol/kew) — the interface and keybindings this
  follows
- [btop](https://github.com/aristocratos/btop) — the banner and the red ramp
- [ytmapi-rs](https://github.com/nick42d/youtui) — the YouTube Music protocol layer
- [ratatui](https://github.com/ratatui/ratatui), [mpv](https://mpv.io),
  [yt-dlp](https://github.com/yt-dlp/yt-dlp) — everything else that does the work
