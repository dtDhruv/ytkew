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
| `mpv: failed to spawn` | `mpv` is not on `PATH`. |
| First track stalls for seconds | Normal: yt-dlp has to resolve the stream. Later tracks are prefetched. |
| Library is empty but you have playlists | Liked Music and Library Songs are different sets. See [Library](/ytkew/guide/library/). |
| A playlist stops at 5,000 tracks | The paging cap. It is there so a runaway continuation cannot spin forever. |
| Media keys do nothing | No session D-Bus. Everything else still works. |
| Visualizer is flat | PipeWire is not running, or nothing is playing through the default sink. |
| Visualizer shows other apps' audio | Expected: it captures the sink monitor, like cava. |

## Reporting a bug

Say which terminal **and** multiplexer you are using. A surprising proportion
of the bugs in this project are one terminal disagreeing with another, and
"kitty 0.35 inside zellij 0.43" is the part that makes a report reproducible.

Issues: [github.com/dtDhruv/ytkew/issues](https://github.com/dtDhruv/ytkew/issues)
