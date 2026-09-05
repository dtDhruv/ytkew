//! ytkew -- a terminal YouTube Music player.
//!
//! # How it fits together
//!
//! | Layer | Module | Responsibility |
//! |---|---|---|
//! | API | [`api`], [`model`] | YouTube Music over `ytmapi-rs`, normalised into [`model::Track`] |
//! | Playback | [`player`], [`queue`] | mpv driven over its JSON IPC socket, and the play order |
//! | State | [`app`] | everything the interface reads; the only mutable owner |
//! | Interface | [`ui`] | ratatui rendering, one module per pane |
//! | Art | [`art`] | kitty / sixel / half-block cover rendering |
//! | Integration | [`mpris`], [`visual`] | D-Bus media controls, PipeWire spectrum tap |
//!
//! # Threading
//!
//! [`app::App`] is single-threaded and owns all interface state. Anything that
//! could block -- API calls, cover downloads -- is spawned onto tokio and
//! reports back through [`app::AppMsg`], so a slow response never stalls a
//! redraw. mpv runs as a separate process for the same reason.
//!
//! # Where to start
//!
//! [`run`] holds the event loop. [`app::input`] maps keys and mouse events to
//! [`config::Action`]s, and [`ui::views::draw`] renders a frame from the
//! current [`app::App`].

//! # A note on the shape of this crate
//!
//! ytkew is an application that happens to expose a library, not a library
//! that happens to ship a binary. `lib.rs` exists so the code can be tested
//! and documented, not so anyone can depend on it. That is why errors are
//! [`anyhow::Error`] throughout rather than a typed enum: nothing downstream
//! needs to match on them, and a binary gains nothing from the ceremony. If
//! that ever changes, the API layer is where typed errors would start.

// Tests assert; unwrapping there is the clearest way to say "this must hold".
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod app;
pub mod art;
pub mod browser;
pub mod cli;
pub mod config;
pub mod desktop;
pub mod model;
pub mod mpris;
pub mod palette;
pub mod player;
pub mod queue;
pub mod run;
pub mod theme;
pub mod ui;
pub mod visual;
