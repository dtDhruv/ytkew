---
title: Signing in
description: Three ways to give ytkew access to your library, and which to pick.
---

**Search, radio, playlists by ID and lyrics need no credentials at all.** Only
your own library — playlists, liked songs, liking tracks — does.

```sh
ytkew --auth browser     # lifts an existing Firefox login
ytkew --auth cookie      # paste a Cookie header from any browser
ytkew --auth oauth       # Google Cloud "TVs and Limited Input devices" client
```

## From the browser

`--auth browser` reads the YouTube cookies straight out of your Firefox
profile, so signing in at music.youtube.com is all the setup there is.

It searches native, snap and flatpak Firefox plus LibreWolf, Waterfox and Zen.
It reads a *snapshot* rather than the live database, since Firefox keeps it
open in WAL mode; deletes that snapshot afterwards; and checks the cookies
against the API before writing anything.

Chromium is not supported. It encrypts cookie values with a key held in the
desktop keyring, which needs both a keyring client and AES-GCM to undo — a
different job. Use `--auth cookie` instead.

## Pasting a header

1. Open music.youtube.com, signed in.
2. Open devtools and pick any request to `music.youtube.com`.
3. Copy the whole `Cookie` **request** header.
4. Run `ytkew --auth cookie` and paste it.

It is validated against the API before it is saved, so a bad paste fails
immediately rather than looking fine until you open your library.

## OAuth

`--auth oauth` runs a device flow, but needs a Google Cloud OAuth client of
type *TVs and Limited Input devices*, which you have to create yourself. Its
tokens also expire and are refreshed on load. Cookie auth is easier and does
not expire on a timer.

:::danger
A YouTube Music cookie header **authenticates as you**. ytkew writes it to
`~/.config/ytkew/cookie.txt` with owner-only permissions and never sends it
anywhere but Google.

Never paste one into a bug report. If one leaks, sign out of Google
everywhere — that revokes it.
:::
