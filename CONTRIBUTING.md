# Contributing to ytkew

Thanks for taking a look. Bug reports, patches, themes and
terminal-compatibility notes are all welcome, and you do not need to know
Rust well to help — a good bug report about a terminal that renders artwork
strangely is worth as much as a patch.

- [Reporting a bug](#reporting-a-bug)
- [Getting set up](#getting-set-up)
- [Where things live](#where-things-live)
- [Making a change](#making-a-change)
- [Conventions](#conventions)
- [Adding a theme](#adding-a-theme)
- [Working on the docs site](#working-on-the-docs-site)

## Reporting a bug

Run this first and paste the output:

```sh
ytkew --diagnose
```

It reports what the terminal says about itself, which credential files are
present, and what each library endpoint returns — which is usually enough to
separate "my library is empty" from "auth is broken" from "this terminal
lies about its cell size".

Also say which terminal and multiplexer you are using. A surprising
proportion of the bugs in this project are one terminal disagreeing with
another, and "kitty 0.35 inside zellij 0.43" is the part that makes a report
reproducible.

**Never paste your `Cookie` header, `cookie.txt` or `oauth.json`.** They
authenticate as you. `--diagnose` deliberately reports only whether those
files exist.

## Getting set up

```sh
git clone https://github.com/dtDhruv/ytkew
cd ytkew
cargo build
cargo test
```

Running it needs `mpv` and `yt-dlp` on `PATH`. **Search works with no
credentials at all**, so most of the app can be developed without signing in;
only the library needs `ytkew --auth browser` or `--auth cookie`.

While developing, point ytkew at a throwaway config so it cannot overwrite
your own settings:

```sh
XDG_CONFIG_HOME=/tmp/ytkew-dev cargo run -- radiohead creep
```

That matters more than it sounds: ytkew rewrites `state.toml` on exit, so a
test run will happily save a volume of zero over your real config.

### Feature flags

`browser-cookies` is on by default and pulls in a bundled SQLite for reading
the Firefox cookie store. Both configurations have to build and lint clean:

```sh
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
```

## Where things live

| Path | What it does |
|---|---|
| `src/run.rs` | the event loop — **start here** |
| `src/app/` | all interface state. `input` maps keys and mouse, `library` the tree, `menu` the overlay, `graphics` album art, `media` D-Bus, `search` the API calls |
| `src/ui/` | rendering, one module per pane, plus `layout` for the geometry the renderer and the graphics painter share |
| `src/art/` | cover rendering: `kitty`, `sixel`, `terminal` probing, `quantize` colours |
| `src/api.rs`, `src/model.rs` | YouTube Music, normalised into one `Track` type |
| `src/player.rs`, `src/queue.rs` | mpv over JSON IPC, and the play order |
| `src/config/` | `Config` is the user's file and is never written; `State` is what the app remembers |
| `src/browser.rs` | reading an existing login out of the Firefox cookie store |
| `docs/` | the documentation site, published to GitHub Pages |

`cargo doc --open` renders the architecture notes in `src/lib.rs`.

### Two things worth knowing before you touch rendering

**Layout goes through `src/ui/layout.rs`, always.** The renderer reserves
cells for album art and a separate pass writes pixels over them. If those two
disagree by even one row, the artwork lands on top of text or leaves the
previous pane's text showing through. Both callers use the same functions for
exactly this reason.

**Cells reserved for graphics are skipped by ratatui's diff**, so nothing is
emitted for them no matter what the terminal is actually showing. Anything
that needs those cells cleared has to write to the terminal directly — see
`clear_cover_art` and `paint_graphics` in `src/app/graphics.rs`.

## Making a change

Before opening a pull request:

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

The tree is kept at zero warnings and zero clippy findings, and CI runs the
same commands plus `cargo audit` and `cargo deny`.

A few things that make a pull request easy to merge:

- **One change per pull request.** A rendering fix and a new feature in the
  same branch take twice as long to review.
- **Say what you observed, not just what you changed.** "The menu rendered
  under the artwork" tells a reviewer what to check; "fix menu" does not.
- **Add a test if the behaviour is testable without a terminal.** Most of it
  is — layout, the queue, the palette, key resolution and the cover-art
  encoders are all pure functions, and ratatui's `TestBackend` renders widgets
  without a tty.
- **Commit messages: a short imperative subject, then why.** The body is where
  the reasoning goes, and it is what the next person reads when they wonder
  why a line exists.

## Formatting and lints

Nothing here is hand-enforced. `rustfmt` is Rust's formatter -- the same role
black plays in Python: official, opinionated, ships with the toolchain. Run
it and clippy before you push; CI runs the same commands.

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test
```

`rustfmt.toml` changes only the line width. The rest is rustfmt's defaults,
deliberately: they are what every Rust project looks like, and diverging buys
nothing.

Lint levels live in `[lints]` in `Cargo.toml`, so `cargo clippy` behaves the
same locally and in CI without anyone remembering flags. The ones worth
knowing about:

| Lint | Why |
|---|---|
| `clippy::unwrap_used`, `expect_used` | A music player has no business panicking. Tests are exempt at the crate root, where asserting is the point. |
| `unsafe_op_in_unsafe_fn` | `unsafe` is used deliberately in `art/terminal.rs` for termios and ioctl. Each block should say why; none should appear unnoticed. |
| `unreachable_pub` | Keeps the public surface honest. |
| `await_holding_lock` | The event loop is async and holds locks around IO; this catches the deadlock before it happens. |
| `dbg_macro`, `todo` | Debugging leftovers. |

The tree is kept at zero warnings under both feature configurations. The
minimum supported Rust version is in `Cargo.toml` as `rust-version`; it
tracks the floor the dependency tree already imposes rather than a number
anyone picked.

## Conventions

Naming is [RFC 430](https://rust-lang.github.io/rfcs/0430-finalizing-naming-conventions.html)
and the compiler enforces it; idiom is whatever clippy accepts. The rest is
this project's own, and worth knowing before you write much:

**Reach for a crate before writing it yourself.** The tree already leans on
`which`, `time`, `anyhow`, `clap` and `dirs` for things that look small until
you need the edge cases. Hand-rolling is a decision that has to earn itself,
and the comment above it should say what it earned — `art/sixel.rs` is
hand-rolled because chafa sizes its output from a TTY query it cannot make
through a pipe, and that reason is written down where the code is.

**Modules use the 2018 layout.** `src/app.rs` alongside `src/app/`, not
`src/app/mod.rs`. Both work; picking one and keeping to it is the point.

**Errors are `anyhow::Result` with `.context()`.** ytkew is an application
that exposes a library for testing, not a library anyone depends on, so there
is no typed error enum. `src/lib.rs` says so, in case that ever changes.

**Nothing in shipping code panics.** `unwrap` and `expect` are both denied
outside tests. If a thing genuinely cannot fail, say why in a comment and
return a `Result` anyway — the player has no business taking itself down.

**Comments explain why, not what.** The code already says what it does. A
comment earns its place when it records a decision, a constraint, or a
non-obvious reason — why kitty is preferred over sixel, why the blanking is
done by hand rather than through the diff. Delete anything that restates the
line below it.

**Tests describe behaviour.** Name them as sentences —
`a_saved_zero_volume_does_not_come_back_silent` — and assert what a user
would notice.

**Terminal quirks belong in comments.** A surprising amount of this code
exists because terminals disagree with each other. When you work around one,
say which terminal and why, and link the upstream issue if there is one.

**Credentials never reach a log, a test fixture, or the repository.** If you
add a code path that touches `cookie.txt` or `oauth.json`, it writes with
owner-only permissions and it does not print the contents.

## Adding a theme

Themes are three colours and need no code:

```toml
# ~/.config/ytkew/themes/ayu.toml
dark   = "#3d4751"   # borders and inactive chrome
mid    = "#b3b1ad"   # secondary text
bright = "#ffcc66"   # accents, titles, the progress bar
```

To propose one as a built-in, add it to `THEMES` in `src/theme.rs`. The tests
there check that every theme reads dark-to-bright and that its accent is
bright enough to see, so a theme that inverts the interface will not merge.

## Working on the docs site

`docs/` is an [Astro Starlight](https://starlight.astro.build) site. Pages are
Markdown under `docs/src/content/docs/`; the sidebar is declared in
`docs/astro.config.mjs`, so a new page needs an entry there to appear.

```sh
cd docs
npm install
npm run dev      # http://localhost:4321/ytkew/
npm run build    # what CI does
```

The `pages` workflow runs `npm ci`, `npm audit --audit-level=high`, the build,
and then a link check over the generated HTML:

```sh
python3 .github/scripts/check_links.py docs/dist --base /ytkew
```

A link to a page that was renamed, or an anchor that no longer exists, fails
the build rather than shipping. The site is served under `/ytkew/`, so
absolute internal links need that prefix — the checker enforces it.

### The wordmark

`docs/src/assets/wordmark.svg` is generated, not hand-drawn:

```sh
python3 .github/scripts/gen_wordmark.py
```

It reads the same ANSI Shadow grid the program draws in its menu and emits
each cell as a vector shape. Pasting the block characters into a `<pre>`
instead only works if the reader's monospace font tiles `█` and `╗╝║═`
exactly — browsers make no such promise, and the letterforms come apart. If
you change the banner in `src/ui/banner.rs`, re-run the generator.
