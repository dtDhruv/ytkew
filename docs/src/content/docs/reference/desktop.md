---
title: Desktop integration
description: MPRIS, media keys, and the now-playing panel.
---

ytkew exposes MPRIS on D-Bus, so media keys work and it appears in the GNOME
or KDE now-playing panel with artwork, position and controls.

Everything the spec asks for is implemented: play, pause, next, previous,
seek, position, volume, shuffle, loop status, and metadata including the
track, artist, album and length.

Artwork is offered as a `file://` URL into `~/.cache/ytkew/`, because most
panels only fetch local files.

## The desktop entry

`make install` puts the desktop entry and icon in place, which is what gives
the panel a name and picture rather than a bare bus id. Installing only the
binary leaves it showing a generic terminal icon.

```sh
make install
```

To do it by hand:

```sh
install -Dm644 ytkew.desktop ~/.local/share/applications/ytkew.desktop
install -Dm644 assets/ytkew.svg \
  ~/.local/share/icons/hicolor/scalable/apps/ytkew.svg
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache -qtf ~/.local/share/icons/hicolor
```

:::note
No session D-Bus means no MPRIS — common over plain ssh, and in some
containers. ytkew reports it once and carries on; everything else still works.
:::
