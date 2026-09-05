---
title: Album art
description: Three renderers, why kitty is preferred, and how to fix sixel.
---

`cover_mode = "auto"` picks the best renderer available:

1. **Kitty graphics.** Placements are sized in *cells*, so the terminal does
   the scaling and no pixel measurement is involved. This is the only protocol
   immune to the sizing problems below, and it is preferred wherever it works:
   kitty, WezTerm, Ghostty, Konsole, and zellij from **0.45.0** onward.
2. **Sixel.** Takes pixel dimensions, so it needs the terminal's cell size in
   pixels. Only used when that size is pinned or successfully measured.
3. **Half-blocks.** Two pixels per cell, drawn into the normal cell grid.
   Always correctly sized, works anywhere, just softer.

Support is confirmed by the protocol's own capability query rather than
guessed from `TERM_PROGRAM` — a wrong guess would dump kilobytes of base64
across the screen as literal text.

`b` shows and hides the art. *Which* renderer is used is a setting, not
something to cycle past on the way to turning it off: set `cover_mode`, or
pick it from the options pane.

## Why sixel is hard

Nothing reliably reports a terminal's cell size in pixels.

- Under zellij, the tty window size gives the **outer window's** pixels with
  the **pane's** cell count, so dividing them is nonsense.
- zellij also has an open bug that renders sixel at double height, which
  nothing the terminal reports reflects.
- A cursor-advance probe measures something real but disagrees with both.

So `auto` only chooses sixel when the size has been pinned by hand, and
half-blocks — always correctly sized — are the safe default.

## Fixing the scale

If sixel art is the wrong size, `[` and `]` resize it by eye while you watch,
and the result is saved. Or pin it directly:

```toml
cell_px = [16, 32]   # whatever your terminal actually uses
```

## Behind the menu

Pixel images are not part of the cell grid, so nothing drawn into the terminal
buffer can cover them — an overlay would render *underneath*. While the escape
menu is open, ytkew takes the image off screen rather than substituting coarse
block art, which would read as the picture going blurry.
