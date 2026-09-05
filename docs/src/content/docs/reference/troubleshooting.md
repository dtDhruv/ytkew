---
title: Troubleshooting
description: What to try first, and the cases that come up most.
---

## Start here

```sh
ytkew --diagnose
```

Reports what the terminal says about itself, which credential files are
present, and what each library endpoint returns — which separates "my library
is empty" from "auth is broken" from "this terminal lies about its cell size".

:::danger
Never paste your `Cookie` header, `cookie.txt` or `oauth.json` into a bug
report. They authenticate as you. `--diagnose` deliberately reports only
whether those files exist.
:::

## Common cases

| Symptom | Cause |
|---|---|
| No sound, everything else looks fine | Check the volume meter in the tab strip. It reads **muted** at zero. |
| Audio sounds fuzzy or clipped | Volume above 100%. See [Configuration](/ytkew/reference/configuration/). |
| Cover art is the wrong size | Sixel with a bad cell size. Press `[` and `]`, or set `cover_mode = "blocks"`. |
| Cover art is missing entirely | The terminal answered no to both capability queries. Half-blocks should still draw — check `cover_enabled`. |
| `failed to spawn mpv` | mpv is not on `PATH`. The error lists install commands. |
| `yt-dlp not found on PATH` | Install it, or set `ytdlp_path`. Nothing plays without it. |
| Tracks fail to load, but yt-dlp is installed | Almost always a stale yt-dlp. See below. |
| First track stalls for seconds | Normal: yt-dlp has to resolve the stream. Later tracks are prefetched. |
| Library is empty but you have playlists | Liked Music and Library Songs are different sets. See [Library](/ytkew/guide/library/). |
| A playlist stops at 5,000 tracks | The paging cap. It is there so a runaway continuation cannot spin forever. |
| Media keys do nothing | No session D-Bus. Everything else still works. |
| Generic icon in the launcher or now-playing panel | The desktop entry is not installed. `ytkew --install-desktop-entry`. |
| Visualizer is flat | PipeWire is not running, or nothing is playing through the default sink. |
| Visualizer shows other apps' audio | Expected: it captures the sink monitor, like cava. |

## When tracks stop playing

This is the most common way ytkew breaks, and it is almost never ytkew.

YouTube changes how streams are signed, and yt-dlp is what keeps up. When it
falls behind, tracks fail to resolve and nothing plays. ytkew notices: after
three failures in a row it checks the version and tells you how old it is.

```
tracks keep failing — yt-dlp 2025.01.10 is over a year old. Run `yt-dlp -U`
```

Update it whichever way you installed it:

```sh
yt-dlp -U                 # self-updating standalone binary
pipx upgrade yt-dlp
sudo apt install --only-upgrade yt-dlp
brew upgrade yt-dlp
```

`ytkew --diagnose` reports the age at any time:

```
playback:
  yt-dlp       /usr/bin/yt-dlp
               2026.08.19 (17 days old)
```

:::note
If ytkew says your yt-dlp **is** current, the tracks themselves are the more
likely problem — deleted, private, or unavailable in your region. A single
failure is ordinary and ytkew just skips it; only a run of them gets blamed
on the extractor.
:::

## Reporting a bug

Say which terminal **and** multiplexer you are using. A surprising proportion
of the bugs in this project are one terminal disagreeing with another, and
"kitty 0.35 inside zellij 0.43" is the part that makes a report reproducible.

Issues: [github.com/dtDhruv/ytkew/issues](https://github.com/dtDhruv/ytkew/issues)
