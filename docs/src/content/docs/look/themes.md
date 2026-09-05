---
title: Themes
description: The ten built-in themes, and writing your own.
---

A theme is three colours, matching the slots the interface uses:

| Slot | Used for |
|---|---|
| `dark` | borders and inactive chrome |
| `mid` | secondary text |
| `bright` | accents, titles, the progress bar |

That is deliberately the same shape the cover-derived palette produces, so a
fixed theme and album colours are interchangeable everywhere.

## Built in

`gruvbox` · `nord` · `dracula` · `catppuccin` · `tokyonight` · `everforest` ·
`rosepine` · `solarized` · `matrix` · `mono`

Plus `cover`, which takes the colours from the artwork. That is the default,
and it is what kew does.

Switch at runtime from `esc` → options → theme. The choice is remembered.

## Writing your own

Drop a TOML file into `~/.config/ytkew/themes/`:

```toml
# ~/.config/ytkew/themes/ayu.toml
dark   = "#3d4751"   # borders and inactive chrome
mid    = "#b3b1ad"   # secondary text
bright = "#ffcc66"   # accents, titles, the progress bar
```

The file name is the theme name, and it appears in the options pane alongside
the built-ins. Both `#rrggbb` and `rrggbb` are accepted; add `name = "..."` to
use a name other than the file name.

Reusing a built-in name **replaces** it, so you can retune a shipped theme
without patching ytkew. `cover` is reserved.

:::note
Themes are read at startup, so restart after adding one. A file that does not
parse is reported on startup and skipped — the rest still load.
:::

## Contrast

Whatever the palette, list text is held above a minimum brightness. Rows used
to be drawn in the border colour, which is fine on solid black and close to
unreadable on a translucent terminal — and a cover-derived palette can push
that colour to nearly nothing.
