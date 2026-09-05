---
title: Layout
description: The split track view and what to put beside the player.
---

On a terminal at least **88 columns** wide the track view splits: the cover,
metadata, visualizer and progress bar keep a column on the left, and the space
that would otherwise sit empty becomes a list.

```toml
side_pane = "queue"           # off | queue | library
```

This is the shape cmus and spotify-tui use — the browser beside the player
rather than under it.

`j`, `k` and `enter` act on the side pane while it is showing, since the
player column has nothing to select. Below 88 columns it collapses back to a
single centred column on its own.

## How the column is sized

The player pane takes a share of the terminal up to a cap. Its contents fill
the pane rather than being tied to the cover's width, so the title and
progress bar get the room even though the artwork cannot grow past its row
cap — and the cover is centred over them.

## The visualizer

```toml
visualizer_mode = "bars"      # bars | braille | off
visualizer_height = 6
visualizer_bar_width = 2
```

Braille packs four levels into each cell, so it reads as smoother at the same
height. `off` reclaims the rows for the rest of the layout.

The spectrum is a real FFT over the PipeWire **sink monitor**, which means it
captures everything the machine is playing, not just ytkew. That is how cava
behaves too.
