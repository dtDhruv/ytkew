"""Regenerate every derived asset from its single source.

Three jobs:

* The wordmark. The terminal draws it with block and box-drawing characters,
  which only line up if the renderer's font tiles them exactly -- browsers do
  not guarantee that, and the letterforms come apart. Emitting the same grid
  as vector shapes gives identical output everywhere.
* The icon. `assets/ytkew.svg` is the only copy anyone should edit; the
  website needs its own under `src/` for Astro's pipeline and another in
  `public/` for the favicon. Copying them here means changing the logo is one
  file, not three that quietly drift.
* The screenshots. `assets/ytkew_*.png` are what the README and the guide
  pages show; the site needs its own copies under `public/`. Same reason as
  the icon: one file to replace when a screenshot is retaken.

Run it after touching the icon, a screenshot, or `src/ui/banner.rs`:

    python3 .github/scripts/gen_assets.py

CI runs it too and fails if the tree changes, so a stale copy cannot ship.
"""
import pathlib
import re
import shutil
import sys

# Every path below is relative to the repository, not to wherever this was
# invoked from -- the previous version of this script assumed a directory and
# only ever worked on one machine.
ROOT = pathlib.Path(__file__).resolve().parents[2]

BANNER_RS = ROOT / "src/ui/banner.rs"


def banner_rows() -> list[str]:
    """Read the wordmark out of the Rust source that draws it.

    Parsing the letter tables is uglier than keeping a copy of the finished
    art next to this script, and it is the reason the two cannot disagree:
    the terminal and the website are then rendering the same characters, and
    editing `banner.rs` is enough to move both.
    """
    src = BANNER_RS.read_text()

    # const Y: [&str; ROWS] = [ "...", "...", ];
    letters = {
        m.group(1): re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(2))
        for m in re.finditer(
            r"const (\w+): \[&str; ROWS\] = \[(.*?)\];", src, re.S
        )
    }
    # const LETTERS: [&[&str; ROWS]; N] = [&Y, &T, ...];
    order = re.search(r"const LETTERS:[^=]*= \[(.*?)\];", src, re.S)
    if not order:
        sys.exit(f"{BANNER_RS}: could not find the LETTERS table")
    names = re.findall(r"&(\w+)", order.group(1))

    missing = [n for n in names if n not in letters]
    if missing:
        sys.exit(f"{BANNER_RS}: LETTERS names {missing} have no letter table")

    heights = {len(letters[n]) for n in names}
    if len(heights) != 1:
        sys.exit(f"{BANNER_RS}: letters disagree on height: {heights}")

    height = heights.pop()
    return ["".join(letters[n][i] for n in names) for i in range(height)]


rows = banner_rows()
COLS = max(len(r) for r in rows)
rows = [r.ljust(COLS) for r in rows]
if len(rows) < 2 or COLS < 10:
    sys.exit(f"parsed a suspiciously small banner: {len(rows)}x{COLS}")

CW, CH = 12.0, 22.0          # cell size; roughly a terminal's aspect
T = 3.2                      # stroke weight for the box-drawing shadow
RAMP = ["#e62525", "#cd2121", "#b31d1d", "#9a1919", "#801414", "#4a0c0c"]

def rect(x, y, w, h):
    return f'<rect x="{x:.2f}" y="{y:.2f}" width="{w:.2f}" height="{h:.2f}"/>'

def shapes(ch, x, y):
    """Cell at (x, y) -> the shapes that character draws."""
    cx, cy = x + CW / 2, y + CH / 2
    half = T / 2
    out = []
    if ch == "█":                    # full block
        # A hair of overlap so adjacent blocks do not show a seam.
        return [rect(x, y, CW + 0.35, CH + 0.35)]
    if ch == "═":                    # horizontal
        out.append(rect(x, cy - half, CW + 0.35, T))
    elif ch == "║":                  # vertical
        out.append(rect(cx - half, y, T, CH + 0.35))
    elif ch == "╗":                  # down and left
        out.append(rect(x, cy - half, cx - x + half, T))
        out.append(rect(cx - half, cy - half, T, y + CH - cy + half))
    elif ch == "╔":                  # down and right
        out.append(rect(cx - half, cy - half, x + CW - cx + half, T))
        out.append(rect(cx - half, cy - half, T, y + CH - cy + half))
    elif ch == "╝":                  # up and left
        out.append(rect(x, cy - half, cx - x + half, T))
        out.append(rect(cx - half, y, T, cy - y + half))
    elif ch == "╚":                  # up and right
        out.append(rect(cx - half, cy - half, x + CW - cx + half, T))
        out.append(rect(cx - half, y, T, cy - y + half))
    return out

# A little breathing room, so no stroke sits flush against the edge.
PAD = 3.0
W, H = COLS * CW + PAD * 2, len(rows) * CH + PAD * 2
parts = [
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W:.0f} {H:.0f}" '
    f'width="{W:.0f}" height="{H:.0f}" role="img" aria-label="ytkew">',
    "  <title>ytkew</title>",
    "  <!-- Generated from the ANSI Shadow banner the program draws in its own",
    "       menu; see .github/scripts/gen_wordmark.py. Vector rather than text",
    "       so the letterforms cannot come apart in a browser's font. -->",
]
for i, row in enumerate(rows):
    cells = []
    for j, ch in enumerate(row):
        cells += shapes(ch, PAD + j * CW, PAD + i * CH)
    if cells:
        parts.append(f'  <g fill="{RAMP[i]}">{"".join(cells)}</g>')
parts.append("</svg>")
# Both the README and the site use it. A code fence would render the banner
# in the reader's default text colour, left-aligned, and only tile correctly
# if their monospace font cooperates -- vector shapes are red, centred and
# identical everywhere.
svg = "\n".join(parts) + "\n"
for out in (ROOT / "assets/wordmark.svg", ROOT / "docs/src/assets/wordmark.svg"):
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(svg)
    print(f"wrote {out.relative_to(ROOT)} ({W:.0f}x{H:.0f})")

# The icon is drawn by hand; these are copies of it. assets/ytkew.svg is the
# one to edit -- the binary embeds that path, and the Makefile installs it.
ICON = ROOT / "assets/ytkew.svg"
for out in (
    ROOT / "docs/src/assets/ytkew.svg",   # Astro's image pipeline
    ROOT / "docs/public/ytkew.svg",       # the favicon
):
    out.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ICON, out)
    print(f"copied {ICON.relative_to(ROOT)} -> {out.relative_to(ROOT)}")

# The screenshots are captures, not something this script can draw. They are
# copied for the same reason as the icon: so retaking one means replacing a
# single file, rather than remembering there is a second copy of each.
# `public/` rather than `src/assets/` because the image service is set to
# passthrough -- Astro would not process it anyway.
shots = sorted(ROOT.glob("assets/ytkew_*.png"))
if not shots:
    sys.exit("no assets/ytkew_*.png; the README and the guide pages need them")
for shot in shots:
    out = ROOT / "docs/public" / shot.name
    out.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(shot, out)
    print(f"copied {shot.relative_to(ROOT)} -> {out.relative_to(ROOT)}")
