---
title: Keys
description: Both keybinding presets, and how to switch between them.
---

kew's bindings, unchanged where they made sense.

| Key | Does |
|---|---|
| `space` `p` | play / pause |
| `h` `l` · `←` `→` | previous / next track |
| `j` `k` · `↑` `↓` | move the selection |
| `a` `d` | seek back / forward |
| `+` `-` | volume |
| `enter` | play the selection |
| `A` | add to the queue without interrupting |
| `P` | play everything in the current view |
| `s` | shuffle |
| `r` | repeat — off → all → one |
| `v` | cycle the visualizer — bars → braille → off |
| `b` | show / hide the cover |
| `.` | like the current track |
| `R` | start radio from the current track |
| `f` `g` | move a track up / down the queue |
| `del` | remove from the queue |
| `bksp` | clear the whole queue |
| `m` | lyrics |
| `/` | search |
| `[` `]` | resize sixel art, if its scale is off |
| `tab` | cycle panes |
| `1`–`4` · `F2`–`F6` | jump to a pane |
| `esc` | menu |
| `q` | quit |

## vim preset

Set `keys = "vim"` in the config, or pick it from `esc` → options → key
bindings. The transport keys stay put; navigation becomes what your fingers
already know.

| Key | Does |
|---|---|
| `j` `k` | move the selection |
| `gg` `G` | top / bottom |
| `ctrl+d` `ctrl+u` | half page |
| `ctrl+f` `ctrl+b` | full page |
| `H` `L` | previous / next track |
| `w` `b` | seek forward / back |
| `J` `K` | move a track down / up the queue |
| `x` `dd` | remove from the queue |
| `o` | play the selection immediately |
| `c` | show / hide the cover |

`h` and `l` are motions here, which is why the transport moves to `H` and `L`.
In the library's column view `h` and `l` step out of and into a level.

Everything not listed is unchanged.

:::tip
The hint line at the bottom of the screen is generated from whichever preset
is active, so what it shows is always the truth — it is not a hardcoded list.
:::
