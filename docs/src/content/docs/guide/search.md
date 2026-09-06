---
title: Search
description: Five filters, and what enter does to each kind of result.
---

YouTube Music filters server-side rather than returning one mixed list, so
ytkew has five filters. `←` and `→` switch between them and re-run the query.

<div class="terminal not-content">
  <div class="terminal-bar"><i></i><i></i><i></i><span>ytkew</span></div>
  <img
    src="/ytkew/ytkew_search.png"
    alt="ytkew's search pane: the query box, the five filter tabs with videos selected, and the results list"
    width="1400"
    height="856"
  />
</div>

| Filter | `enter` does |
|---|---|
| songs, videos | plays it |
| albums, playlists | loads the whole thing and makes it the queue |
| artists | lists their releases |

`A` adds to the queue instead of playing, which is how you stack things up
without interrupting what is on.

## The search box

`/` or `4` opens it. Autocomplete appears as you type. `enter` searches, `esc`
leaves the box, and `i` re-enters it.

Suggestions are tagged with the query that produced them, so a slow reply
cannot overwrite newer typing — and results are tagged with their filter, so
switching filters mid-flight never lands the wrong list.

## Radio

`autoplay_radio` is on by default: playing a single search hit appends
YouTube's radio mix behind it, which is what turns a one-song search into a
listening session rather than three minutes and silence. `R` starts a radio
from whatever is playing.
