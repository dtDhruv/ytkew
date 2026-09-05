---
title: Introduction
description: What ytkew is, what it needs, and what it does not.
---

ytkew plays music from your YouTube Music account without a browser. It takes
its interface and keybindings from [kew](https://github.com/ravachol/kew), and
its playback from mpv — so a slow redraw can never stutter the audio.

## Requirements

| Needs | For |
|---|---|
| `mpv` | playback. Required. |
| `yt-dlp` | resolving stream URLs. Required. |
| A truecolor terminal | cover art and themes. |
| PipeWire | the visualizer only. Everything else works without it. |
| A YouTube Music account | your own library only. Search, radio and lyrics need no credentials. |

## What it is not

- **Not a downloader.** It streams, like the web player does.
- **Not official.** It talks to YouTube Music's internal API through
  [ytmapi-rs](https://github.com/nick42d/youtui), which can change at any
  time. When YouTube breaks something, the fix usually lands there first.
- **Not affiliated with YouTube or Google** in any way.

## Where to go next

- [Install](/ytkew/start/install/) — including the desktop entry and icon
- [Signing in](/ytkew/start/signing-in/) — three ways, one of them automatic
- [Keys](/ytkew/guide/keys/) — both presets
