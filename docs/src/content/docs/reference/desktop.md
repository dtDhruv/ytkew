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

The panel shows a name and picture instead of a bare bus id only once the
desktop entry and icon are installed. Either of these does it:

```sh
make install                      # building from source
ytkew --install-desktop-entry     # after `cargo install ytkew`
```

The second exists because `cargo install` copies the executable and nothing
else. ytkew carries both files inside the binary and writes them to
`~/.local/share`, honouring `XDG_DATA_HOME`. `--uninstall-desktop-entry`
removes them, and `--diagnose` reports which state you are in.

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
