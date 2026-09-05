---
title: Install
description: Building ytkew, and putting the desktop entry and icon in place.
---

## 1. Dependencies

Two programs have to be on your `PATH`, and ytkew will refuse to start
without them:

- **mpv** does the playing. ytkew drives it over a socket rather than
  decoding audio itself, so a slow redraw can never stutter the sound.
- **yt-dlp** turns a YouTube video into a playable stream URL. mpv shells out
  to it for every track. [Why it is needed](#why-yt-dlp), below.

Optionally, **PipeWire** for the spectrum visualizer — everything else works
without it.

### Debian, Ubuntu, Mint

```sh
sudo apt install mpv yt-dlp
```

`yt-dlp` in the Debian and Ubuntu archives is often months behind, and a stale
yt-dlp is the usual cause of tracks failing to load. If that happens:

```sh
sudo apt install pipx && pipx install yt-dlp
```

### Fedora, RHEL

```sh
sudo dnf install mpv yt-dlp
```

### Arch, Manjaro

```sh
sudo pacman -S mpv yt-dlp
```

### openSUSE

```sh
sudo zypper install mpv yt-dlp
```

### Alpine

```sh
doas apk add mpv yt-dlp
```

### macOS

```sh
brew install mpv yt-dlp
```

### Any distribution

yt-dlp ships a self-contained binary that needs no Python:

```sh
sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
  -o /usr/local/bin/yt-dlp && sudo chmod a+rx /usr/local/bin/yt-dlp
```

### Rust

A stable toolchain, if you do not already have one:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 2. Build and install

```sh
git clone https://github.com/dtDhruv/ytkew
cd ytkew
make install
```

That puts the binary in `~/.local/bin` and the desktop entry and icon under
`~/.local/share`. For a system-wide install:

```sh
PREFIX=/usr/local sudo make install
```

`make uninstall` removes all three.

### Binary only

```sh
cargo build --release
install -Dm755 target/release/ytkew ~/.local/bin/ytkew
```

:::note
`cargo install` places only the binary, so ytkew will show a generic icon in
your launcher and in the now-playing panel. `make install` is what puts the
real one in place.
:::

## 3. Build features

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

Reports which of mpv and yt-dlp it can find, what the terminal says about
itself, which credential files are present, and what each library endpoint
returns:

```
playback:
  yt-dlp       /usr/bin/yt-dlp
  mpv          mpv 0.37.0
```

`MISSING` against either of those means nothing will play.

If yours lives somewhere unusual, point at it directly:

```toml
# ~/.config/ytkew/config.toml
ytdlp_path = "/opt/bin/yt-dlp"
```

`youtube-dl` is accepted as a fallback when yt-dlp is not there, though it is
much slower and breaks more often.

## Why yt-dlp

ytkew talks to YouTube Music through a Rust library, but that library only
answers *what exists* — search results, playlists, your library, lyrics.
It cannot give you a URL you can actually play.

Getting that URL means defeating three things YouTube does on purpose: the
stream signature is scrambled and has to be unscrambled by running a
JavaScript function pulled out of YouTube's player; a query parameter has to
be transformed by another one, or you get throttled to a trickle; and
increasingly a proof-of-origin token is demanded on top. All of it changes
often.

Keeping up with that is essentially yt-dlp's whole reason to exist, and no
Rust library does it — the closest one has not had a working fix in over a
year. Shelling out to the tool that is actually maintained beats
reimplementing it badly.
