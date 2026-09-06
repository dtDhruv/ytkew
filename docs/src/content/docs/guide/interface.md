---
title: The interface
description: The four panes, and how to move between them.
---

Four panes, numbered in the tab strip.

<div class="terminal not-content">
  <div class="terminal-bar"><i></i><i></i><i></i><span>ytkew radiohead creep</span></div>
<pre> ytkew   1 queue    2 library    3 track    4 search               not signed in  1/20  vol ▪▪▪▫▫ 60%
──────────────────────────────┘           └───────────────────────────────────────────────────────────
╭| now playing |─────────────────────────╮ ╭| up next |──────────────────────────────────────────────╮
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Creep (Acoustic)               4:19↑│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Karma Police                   4:22█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Let Down                       5:00█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          No Surprises                   3:49█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Kelly Clarkson     Creep (Live)                   4:09█│
│              ▀▀▀▀▀▀▀▀▀▀▀▀              │ │  Radiohead          Fake Plastic Trees             4:51█│
│                                        │ │  Radiohead          Everything In Its Right Place  4:12█│
│ Creep                                  │ │  Scala &amp; Kolacny B… Creep                          4:54█│
│ Radiohead                              │ │  Radiohead          Jigsaw Falling Into Place      4:09║│
│ Creep                                  │ │  Scott Bradlee&#x27;s P… Creep (feat. Haley Reinhart)   4:44║│
│                                        │ │  Radiohead          Exit Music (For A Film)        4:28║│
│                                        │ │  Radiohead          Street Spirit (Fade Out)       4:14║│
│    ▄▅▅▇▇▁                              │ │  Radiohead          Airbag                         4:48║│
│ ▆▆▆██████▇▇                            │ │  Radiohead          You And Whose Army?            3:11║│
│ ███████████▇▅                          │ │  Radiohead          All I Need                     3:49║│
│ ██████████████▅▃▄▃▃▁▂▅▁▃▆▅▁▂▁▂▁▂▁▂▂▁▂▂ │ │  Radiohead          No Surprises                   3:50║│
│ 0:13 ━━●━━━━━━━━━━━━━━━━━━━━━━━━━ 3:58 │ │  Stone Temple Pilo… Creep                          5:33↓│
╰────────────────────────────────| 1/20 |╯ ╰─────────────────────────────────────────────────────────╯
 space play · h/l skip · a/d seek · +/- vol · s shuffle · r repeat · m lyrics · esc menu   F6 help</pre>
</div>

| Key | Pane |
|---|---|
| `1` | queue |
| `2` | library |
| `3` | track |
| `4` | search |

`tab` cycles them, `esc` opens the menu, `F6` shows the help.

## The tab strip

The right-hand end carries the state you need at a glance: whether you are
signed in, position in the queue, shuffle and repeat, and the volume meter.

The volume meter is a five-cell bar. It reads **muted** in the accent colour
at zero rather than a quiet `0%`, because a player that looks like it is
working while making no sound is the single most confusing thing it can do.

## The mouse

The mouse works throughout:

- **Click a tab** to switch panes
- **Click a row** to select it, click again to play it — the same feel as a
  file manager
- **Drag the progress bar** to seek
- **Scroll the wheel** over a list to scroll it; away from a list it changes
  the volume
- **Right-click** anywhere to play or pause

## The menu

`esc` opens a floating menu with the ytkew banner, in the style btop uses.
Options, help and quit. The options pane steps each setting with `←` and `→`
and applies it immediately, so you can see what a theme or a renderer looks
like before committing it to the config.
