---
title: The queue
description: Why the queue behaves the way YouTube Music's does.
---

The queue is not a plain append-only list. It remembers what filled it, and
that decides what happens when you play something else.

<div class="terminal not-content">
  <div class="terminal-bar"><i></i><i></i><i></i><span>ytkew</span></div>
  <img
    src="/ytkew/ytkew_queue.png"
    alt="ytkew's queue pane: the running order with the current track highlighted at the top"
    width="1400"
    height="856"
  />
</div>

| You play | What happens |
|---|---|
| A track from the playlist already on | Moves within the queue. The running order and anything queued behind it survive. |
| A track from a different playlist or album | Replaces the queue with that one, starting where you picked. |
| A one-off from search, mid-playlist | Slots in as the next track and plays. The playlist resumes right after it. |
| `A` on anything | Appends to the end. Never interrupts. |

That third row is the one worth knowing. Hearing one song from search used to
mean losing your playlist; now the song plays and the playlist picks up
immediately afterwards.

## Shuffle

The visible list and the playback order are kept separate. Turning shuffle on
never scrambles the list on screen, and turning it off restores the original
sequence rather than an approximation of it.

A track inserted with "play next" lands after the current one in *both*, so it
really is next even when shuffled.

## Gapless

ytkew keeps two tracks in mpv's playlist at all times: the current one and the
next. Every YouTube track needs a yt-dlp round trip to resolve, which takes
one to seven seconds cold, so without prefetching every transition would
stall. With it, only the very first track waits and skipping is instant.
