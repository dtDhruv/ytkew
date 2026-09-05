---
title: Install
description: Building ytkew, and putting the desktop entry and icon in place.
---

## From source

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

## Binary only

```sh
cargo build --release
install -Dm755 target/release/ytkew ~/.local/bin/ytkew
```

:::note
`cargo install` places only the binary, so ytkew will show a generic icon in
your launcher and in the now-playing panel. `make install` is what puts the
real one in place.
:::

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

Reports what the terminal says about itself, which credential files are
present, and what each library endpoint returns.
