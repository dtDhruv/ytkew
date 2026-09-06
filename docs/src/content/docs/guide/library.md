---
title: Library
description: Browsing playlists, albums and artists as columns.
---

The library draws each level as its own column, in the manner of a file
manager, so the path you walked stays on screen instead of being implied by
indentation.

<div class="terminal not-content">
  <div class="terminal-bar"><i></i><i></i><i></i><span>ytkew</span></div>
  <img
    src="/ytkew/ytkew_library.png"
    alt="ytkew's library as columns: the top level, artists, one artist's albums, and that album's tracks"
    width="1400"
    height="856"
  />
</div>

| Key | Does |
|---|---|
| `←` `→` (`h` `l` in vim) | step out of / into a level |
| `j` `k` | move within a level |
| `enter` | step in, or play a track in its playlist's context |
| `A` | add the whole container to the queue |
| `P` | play everything under the cursor |

The rightmost column previews what is inside the entry under the cursor, so
you can see a playlist's contents before entering it.

```toml
library_layout = "columns"    # columns | tree
```

A pane too narrow for two columns falls back to an indented tree on its own —
which is what happens in the track view's side pane and on a small terminal.

## Liked Music is not Library Songs

They are different sets, and this trips people up. **Liked Music** is the `LM`
auto-playlist: everything you have thumbed up. **Library Songs** is what you
explicitly added to your library. Plenty of accounts have hundreds of the
first and none of the second.

## Loading

Everything loads on demand, one level at a time. Long playlists arrive page by
page and appear as they land, so the first hundred tracks show immediately
rather than after the last round trip. A "play all" starts on the first page
and the queue is extended behind it as the rest arrive.

Paged fetches stop at 5,000 items, so a playlist longer than that is
truncated. The cap exists so that a continuation token which keeps pointing at
more results cannot spin forever.
