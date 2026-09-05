"""Turn the ANSI Shadow banner into an SVG.

The terminal draws it with block and box-drawing characters, which only line
up if the renderer's font tiles them exactly -- browsers do not guarantee
that, and the letterforms come apart. Emitting the same grid as vector shapes
gives identical output everywhere.
"""
import pathlib

S = "/tmp/claude-1001/-home-dhruv-codes/95205970-2abd-4d47-96e4-f047cdb41886/scratchpad/"
rows = pathlib.Path(S + "banner.txt").read_text().rstrip("\n").split("\n")
COLS = max(len(r) for r in rows)
rows = [r.ljust(COLS) for r in rows]

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
# Only the website needs this. The README shows the banner as text in a code
# fence, where GitHub's monospace font tiles the block characters correctly --
# it is arbitrary web pages that cannot be trusted to.
out = pathlib.Path("docs/src/assets/wordmark.svg")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text("\n".join(parts) + "\n")
print(f"wrote {out} ({W:.0f}x{H:.0f})")
