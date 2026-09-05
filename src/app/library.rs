//! The lazily-loaded library tree: playlists, artists, albums and their
//! tracks, fetched a level at a time as nodes are opened.

use crate::api::{AlbumRef, ArtistRef, Playlist};
use crate::model::Track;
use anyhow::Result;

use super::*;

/// What a library tree node represents.
#[derive(Clone, Debug)]
pub enum LibKind {
    /// The `LM` auto-playlist: songs liked *in YouTube Music*.
    LikedMusic,
    /// Songs explicitly added to your library -- a different set entirely,
    /// and commonly empty even when Liked Music is not.
    LibrarySongs,
    ArtistsFolder,
    AlbumsFolder,
    PlaylistsFolder,
    Playlist(Playlist),
    Artist(ArtistRef),
    Album(AlbumRef),
    Song(Track),
}

impl LibKind {
    pub(crate) fn is_song(&self) -> bool {
        matches!(self, LibKind::Song(_))
    }

    pub(crate) fn label(&self) -> String {
        match self {
            LibKind::LikedMusic => "Liked Music".into(),
            LibKind::LibrarySongs => "Library Songs".into(),
            LibKind::ArtistsFolder => "Artists".into(),
            LibKind::AlbumsFolder => "Albums".into(),
            LibKind::PlaylistsFolder => "Playlists".into(),
            LibKind::Playlist(p) => p.title.clone(),
            LibKind::Artist(a) => a.name.clone(),
            LibKind::Album(a) => a.title.clone(),
            LibKind::Song(t) => {
                if t.artist.is_empty() {
                    t.title.clone()
                } else {
                    format!("{} — {}", t.artist, t.title)
                }
            }
        }
    }

    pub(crate) fn sublabel(&self) -> String {
        match self {
            LibKind::Playlist(p) => {
                if p.author.is_empty() {
                    p.track_count.clone()
                } else {
                    format!("{} · {}", p.track_count, p.author)
                }
            }
            LibKind::Artist(a) => a.subtitle.clone(),
            LibKind::Album(a) => a.year.clone(),
            LibKind::Song(t) => t.duration_text.clone(),
            _ => String::new(),
        }
    }
}

/// A node in the lazily-loaded library tree.
pub struct LibNode {
    pub kind: LibKind,
    pub expanded: bool,
    pub loading: bool,
    /// Whether children have been fetched. Distinguishes "not yet loaded"
    /// from "loaded and genuinely empty".
    pub loaded: bool,
    pub children: Vec<LibNode>,
}

impl LibNode {
    pub(crate) fn new(kind: LibKind) -> Self {
        let loaded = kind.is_song();
        Self {
            kind,
            expanded: false,
            loading: false,
            loaded,
            children: Vec::new(),
        }
    }
}

/// One rendered line of the tree, precomputed so the view does no traversal
/// and no allocation per frame.
pub struct LibRow {
    pub path: Vec<usize>,
    pub depth: usize,
    pub label: String,
    pub sublabel: String,
    /// Disclosure indicator: expanded, collapsed, loading, or a leaf.
    pub marker: &'static str,
    pub is_song: bool,
}

/// One column of the miller view: the siblings at a single depth.
///
/// Derived from the same tree the stacked view walks, so the two are always
/// showing the same library -- only the arrangement differs.
pub struct LibColumn {
    /// Path to each entry, so a click resolves straight back to a node.
    pub rows: Vec<Vec<usize>>,
    /// Which entry the cursor passes through, if it passes through this
    /// column at all. The preview column on the right has none.
    pub selected: Option<usize>,
}

impl App {
    /// Path from the root to the cursor.
    pub(crate) fn library_cursor(&self) -> Vec<usize> {
        self.library_rows
            .get(self.library_sel)
            .map(|r| r.path.clone())
            .unwrap_or_default()
    }

    /// Where a path sits in the stacked row list, which is what `library_sel`
    /// indexes. `None` once an ancestor is collapsed.
    pub(crate) fn library_row_index(&self, path: &[usize]) -> Option<usize> {
        self.library_rows.iter().position(|r| r.path == path)
    }

    /// The chain of sibling lists from the root down to the cursor, plus a
    /// preview of what is inside the focused node.
    pub(crate) fn library_columns(&self) -> Vec<LibColumn> {
        let cursor = self.library_cursor();
        let mut cols = Vec::new();
        let mut level: &[LibNode] = &self.library;
        let mut prefix: Vec<usize> = Vec::new();
        // One past the cursor, so the last iteration yields the preview.
        for depth in 0..=cursor.len() {
            if level.is_empty() {
                break;
            }
            let selected = cursor.get(depth).copied();
            cols.push(LibColumn {
                rows: (0..level.len())
                    .map(|i| {
                        let mut p = prefix.clone();
                        p.push(i);
                        p
                    })
                    .collect(),
                selected,
            });
            let Some(i) = selected else { break };
            let Some(node) = level.get(i) else { break };
            prefix.push(i);
            level = &node.children;
        }
        cols
    }

    /// Move the cursor among its siblings, staying at the same depth. In the
    /// stacked view the same keys walk into children; in columns that would
    /// jump the cursor sideways out of the list you are reading.
    pub(crate) fn library_move_sibling(&mut self, delta: isize) {
        let cursor = self.library_cursor();
        let Some((&last, parent)) = cursor.split_last() else {
            return;
        };
        let count = match self.node_at(parent) {
            Some(node) => node.children.len(),
            None if parent.is_empty() => self.library.len(),
            None => return,
        };
        if count == 0 {
            return;
        }
        let next = (last as isize + delta).clamp(0, count as isize - 1) as usize;
        let mut path = parent.to_vec();
        path.push(next);
        if let Some(i) = self.library_row_index(&path) {
            self.library_sel = i;
        }
    }

    /// Jump to the first or last sibling.
    pub(crate) fn library_edge_sibling(&mut self, last: bool) {
        self.library_move_sibling(if last { isize::MAX / 2 } else { isize::MIN / 2 });
    }

    /// Step out to the parent column.
    pub(crate) fn library_ascend(&mut self) {
        let cursor = self.library_cursor();
        if cursor.len() < 2 {
            return;
        }
        let parent = cursor[..cursor.len() - 1].to_vec();
        if let Some(i) = self.library_row_index(&parent) {
            self.library_sel = i;
        }
    }

    /// Step into the focused node, loading it first if need be.
    pub(crate) fn library_descend(&mut self) {
        let path = self.library_cursor();
        let Some(node) = self.node_at(&path) else {
            return;
        };
        if node.kind.is_song() {
            return;
        }
        if !node.loaded {
            // Same path the stacked view takes: mark it loading and fetch.
            if !node.loading {
                let kind = node.kind.clone();
                if let Some(n) = self.node_at_mut(&path) {
                    n.loading = true;
                }
                self.rebuild_library_rows();
                self.spawn_library_load(path, kind);
            }
            return;
        }
        if node.children.is_empty() {
            return;
        }
        if let Some(n) = self.node_at_mut(&path) {
            n.expanded = true;
        }
        self.rebuild_library_rows();
        let mut child = path;
        child.push(0);
        if let Some(i) = self.library_row_index(&child) {
            self.library_sel = i;
        }
    }

    /// Build the root of the tree. Children load on demand.
    pub fn ensure_library(&mut self) {
        if !self.library.is_empty() {
            return;
        }
        if !self.api.is_authenticated() {
            self.notify("library needs auth -- run `ytkew --auth cookie`");
            return;
        }
        self.library = vec![
            LibNode::new(LibKind::LikedMusic),
            LibNode::new(LibKind::LibrarySongs),
            LibNode::new(LibKind::ArtistsFolder),
            LibNode::new(LibKind::AlbumsFolder),
            LibNode::new(LibKind::PlaylistsFolder),
        ];
        self.rebuild_library_rows();
    }

    // --- library ----------------------------------------------------------

    pub(crate) fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut LibNode> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.library.get_mut(first)?;
        for &i in rest {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }

    /// Flatten the visible tree into display rows. Called only when the tree
    /// changes, so rendering stays allocation-free.
    pub fn rebuild_library_rows(&mut self) {
        fn walk(nodes: &[LibNode], path: &mut Vec<usize>, depth: usize, out: &mut Vec<LibRow>) {
            for (i, node) in nodes.iter().enumerate() {
                path.push(i);
                let marker = if node.kind.is_song() {
                    " "
                } else if node.loading {
                    "\u{22ef}"
                } else if node.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                out.push(LibRow {
                    path: path.clone(),
                    depth,
                    label: node.kind.label(),
                    sublabel: node.kind.sublabel(),
                    marker,
                    is_song: node.kind.is_song(),
                });
                if node.expanded {
                    walk(&node.children, path, depth + 1, out);
                }
                path.pop();
            }
        }
        let mut rows = Vec::new();
        walk(&self.library, &mut Vec::new(), 0, &mut rows);
        self.library_rows = rows;
        if self.library_sel >= self.library_rows.len() {
            self.library_sel = self.library_rows.len().saturating_sub(1);
        }
    }

    /// Enter/expand the selected row, or play it if it is a song.
    pub(crate) fn activate_library_row(&mut self, jump: bool) {
        let Some(row) = self.library_rows.get(self.library_sel) else {
            return;
        };
        let path = row.path.clone();

        // Songs play in the context of their siblings, so selecting track 4
        // of an album queues the whole album from there.
        if row.is_song {
            let Some((parent_path, &index)) = path.split_last().map(|(i, p)| (p, i)) else {
                return;
            };
            let siblings: Vec<Track> = match self.node_at(parent_path) {
                Some(parent) => parent
                    .children
                    .iter()
                    .filter_map(|c| match &c.kind {
                        LibKind::Song(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect(),
                None => return,
            };
            if siblings.is_empty() {
                return;
            }
            if jump || self.queue.current().is_none() {
                let start = index.min(siblings.len() - 1);
                self.play_all(siblings, start);
            } else if let Some(t) = siblings.get(index).cloned() {
                self.enqueue_track(t, false);
            }
            return;
        }

        // alt+enter on a container plays its whole contents rather than
        // just opening it.
        if jump {
            self.play_selected_library_node();
            return;
        }
        // In columns, enter means the same as stepping right. Toggling the
        // node shut would collapse the level the cursor is standing in.
        if self.in_library_columns() {
            self.library_descend();
            return;
        }
        // Containers: collapse if open, expand if already fetched, else load.
        let Some(node) = self.node_at_mut(&path) else {
            return;
        };
        if node.expanded {
            node.expanded = false;
            self.rebuild_library_rows();
            return;
        }
        if node.loaded {
            node.expanded = true;
            self.rebuild_library_rows();
            return;
        }
        if node.loading {
            return;
        }
        node.loading = true;
        let kind = node.kind.clone();
        self.rebuild_library_rows();
        self.spawn_library_load(path, kind);
    }

    /// Fetch a node's children off-thread, replying with its path.
    ///
    /// Playlists and library sections arrive a page at a time and are
    /// forwarded as they land, so a few thousand tracks show their first
    /// hundred immediately instead of after every round trip has finished.
    pub(crate) fn spawn_library_load(&self, path: Vec<usize>, kind: LibKind) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut sent = 0usize;
            let mut send = |children: Vec<LibKind>| {
                let msg = AppMsg::LibChildren {
                    path: path.clone(),
                    children,
                    first: sent == 0,
                    last: false,
                };
                sent += 1;
                let _ = tx.send(msg);
            };

            let result: Result<()> = match &kind {
                LibKind::LikedMusic => {
                    api.liked_songs_paged(|t| send(t.into_iter().map(LibKind::Song).collect()))
                        .await
                }
                LibKind::LibrarySongs => {
                    api.library_songs_paged(|t| send(t.into_iter().map(LibKind::Song).collect()))
                        .await
                }
                LibKind::PlaylistsFolder => {
                    api.library_playlists_paged(|p| {
                        send(p.into_iter().map(LibKind::Playlist).collect())
                    })
                    .await
                }
                LibKind::ArtistsFolder => {
                    api.library_artists_paged(|a| {
                        send(a.into_iter().map(LibKind::Artist).collect())
                    })
                    .await
                }
                LibKind::AlbumsFolder => {
                    api.library_albums_paged(|a| send(a.into_iter().map(LibKind::Album).collect()))
                        .await
                }
                LibKind::Playlist(p) => {
                    api.playlist_tracks_paged(&p.id, |t| {
                        send(t.into_iter().map(LibKind::Song).collect())
                    })
                    .await
                }
                // Artist pages and albums come back whole; there is nothing
                // to page through.
                LibKind::Artist(a) => api
                    .artist_albums(&a.channel_id)
                    .await
                    .map(|al| send(al.into_iter().map(LibKind::Album).collect())),
                LibKind::Album(a) => api
                    .album_tracks(&a.id)
                    .await
                    .map(|t| send(t.into_iter().map(LibKind::Song).collect())),
                LibKind::Song(_) => Ok(()),
            };

            let msg = match result {
                // Always close the fetch, even if no page arrived: this is
                // what clears the node's loading flag and what tells an empty
                // section it really is empty.
                Ok(()) => AppMsg::LibChildren {
                    path,
                    children: Vec::new(),
                    first: sent == 0,
                    last: true,
                },
                Err(e) => AppMsg::LibFailed {
                    path,
                    error: format!("load failed: {e}"),
                },
            };
            let _ = tx.send(msg);
        });
    }

    /// Every song directly under a node, in display order.
    pub(crate) fn songs_under(&self, path: &[usize]) -> Vec<Track> {
        let Some(node) = self.node_at(path) else {
            return Vec::new();
        };
        node.children
            .iter()
            .filter_map(|c| match &c.kind {
                LibKind::Song(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    /// Play a library node's whole contents, fetching them first if needed.
    pub(crate) fn play_selected_library_node(&mut self) {
        let Some(row) = self.library_rows.get(self.library_sel) else {
            return;
        };
        let path = row.path.clone();
        // A song plays its siblings from that point, which is what "play all"
        // means when the cursor is inside a list.
        if row.is_song {
            self.activate_selection(true);
            return;
        }
        let Some(node) = self.node_at(&path) else {
            return;
        };
        if node.loaded {
            let tracks = self.songs_under(&path);
            if tracks.is_empty() {
                self.notify("nothing playable here");
            } else {
                self.play_all(tracks, 0);
            }
            return;
        }
        // Not fetched yet: request it and play once it lands.
        if let Some(n) = self.node_at_mut(&path) {
            if n.loading {
                return;
            }
            n.loading = true;
        }
        let kind = self.node_at(&path).map(|n| n.kind.clone());
        if let Some(kind) = kind {
            self.pending_play = Some(path.clone());
            self.notify("loading…");
            self.rebuild_library_rows();
            self.spawn_library_load(path, kind);
        }
    }
}
