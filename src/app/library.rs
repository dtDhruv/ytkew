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

impl App {
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
    pub(crate) fn spawn_library_load(&self, path: Vec<usize>, kind: LibKind) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result: Result<Vec<LibKind>> = match &kind {
                LibKind::LikedMusic => api
                    .liked_songs()
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::LibrarySongs => api
                    .library_songs()
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::PlaylistsFolder => api
                    .library_playlists()
                    .await
                    .map(|p| p.into_iter().map(LibKind::Playlist).collect()),
                LibKind::ArtistsFolder => api
                    .library_artists()
                    .await
                    .map(|a| a.into_iter().map(LibKind::Artist).collect()),
                LibKind::AlbumsFolder => api
                    .library_albums()
                    .await
                    .map(|a| a.into_iter().map(LibKind::Album).collect()),
                LibKind::Playlist(p) => api
                    .playlist_tracks(&p.id)
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::Artist(a) => api
                    .artist_albums(&a.channel_id)
                    .await
                    .map(|al| al.into_iter().map(LibKind::Album).collect()),
                LibKind::Album(a) => api
                    .album_tracks(&a.id)
                    .await
                    .map(|t| t.into_iter().map(LibKind::Song).collect()),
                LibKind::Song(_) => Ok(Vec::new()),
            };
            let msg = match result {
                Ok(children) => AppMsg::LibChildren { path, children },
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
