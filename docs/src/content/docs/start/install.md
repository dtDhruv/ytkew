---
title: Install
description: Installing ytkew on Linux and macOS.
---

ytkew needs two programs on your `PATH` and refuses to start without them:

- **mpv** 0.30 or newer, which does the playing
- **yt-dlp**, which turns a YouTube video into a playable stream

Get those first, then pick an install method.

## Dependencies

### Linux

```sh
sudo apt install mpv yt-dlp        # Debian, Ubuntu, Mint
sudo dnf install mpv yt-dlp        # Fedora, RHEL
sudo pacman -S mpv yt-dlp          # Arch, Manjaro
sudo zypper install mpv yt-dlp     # openSUSE
doas apk add mpv yt-dlp            # Alpine
```

PipeWire, if you want the spectrum visualizer. Everything else works without
it.

:::caution
`yt-dlp` in the Debian and Ubuntu archives is often months behind, and a stale
yt-dlp is the usual reason tracks fail to load. If that happens:

```sh
sudo apt install pipx && pipx install yt-dlp
```
:::

### macOS

```sh
brew install mpv yt-dlp
```

:::note[Two features are Linux-only]
- **The spectrum visualizer** reads audio through PipeWire, which macOS does
  not have, so it stays flat. Set `visualizer_mode = "off"` to reclaim the
  rows.
- **Media keys and the now-playing panel** use MPRIS over D-Bus, also
  Linux-only.

Everything else — playback, cover art, the library, search, themes — works
the same.
:::

## Install

### From crates.io

The shortest route on either platform:

```sh
cargo install ytkew
ytkew --install-desktop-entry
```

`cargo install` copies the executable and nothing else, so that second line is
what gives you a launcher entry and a real icon instead of a generic one.
`ytkew --uninstall-desktop-entry` reverses it.

Needs a Rust toolchain, if you do not have one:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### From source

```sh
git clone https://github.com/dtDhruv/ytkew
cd ytkew
make install
```

Binary into `~/.local/bin`, desktop entry and icon into `~/.local/share` — no
separate `--install-desktop-entry` step. Use `PREFIX=/usr/local sudo make
install` for a system-wide install, and `make uninstall` to remove all three.

### Without a package manager

yt-dlp ships a self-contained binary that needs no Python:

```sh
sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
  -o /usr/local/bin/yt-dlp && sudo chmod a+rx /usr/local/bin/yt-dlp
```

## Build features

`browser-cookies` is on by default and pulls in a bundled SQLite so
[`ytkew --auth browser`](/ytkew/start/signing-in/) can read the Firefox cookie
store. For a leaner build without it:

```sh
cargo build --release --no-default-features
```

## Checking it works

```sh
ytkew --diagnose
```

```
playback:
  yt-dlp       /usr/bin/yt-dlp
               2026.08.19 (17 days old)
  mpv          mpv 0.37.0
```

`MISSING` against either means nothing will play. If yours lives somewhere
unusual, point at it directly:

```toml
# ~/.config/ytkew/config.toml
ytdlp_path = "/opt/bin/yt-dlp"
```

`youtube-dl` is accepted as a fallback, though it is slower and breaks more
often.

## Keeping yt-dlp current

YouTube changes how streams are signed and yt-dlp is what keeps up, so **a
stale yt-dlp is the most common reason a working ytkew stops playing.** After
three failures in a row ytkew checks the version and tells you how old it is.

```sh
yt-dlp -U                 # self-updating standalone binary
pipx upgrade yt-dlp
sudo apt install --only-upgrade yt-dlp
brew upgrade yt-dlp
```
