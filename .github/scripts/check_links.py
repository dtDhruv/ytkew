#!/usr/bin/env python3
"""Check that every internal link in a built site resolves.

A dead anchor or a link to a page that was renamed is invisible until someone
clicks it, so fail the build instead. Runs against generated output, which is
why it takes a directory rather than knowing anything about the source.

    python3 .github/scripts/check_links.py docs/dist --base /ytkew
"""

import argparse
import pathlib
import re
import sys

EXTERNAL = ("http://", "https://", "mailto:", "data:", "tel:", "javascript:")


def resolves(dist: pathlib.Path, rel: str) -> bool:
    """A path resolves if it is a file, a directory with an index, or gains .html."""
    target = dist / rel
    if target.is_file():
        return True
    if (target / "index.html").is_file():
        return True
    return (dist / (rel.rstrip("/") + ".html")).is_file()


def check(dist: pathlib.Path, base: str) -> list[str]:
    problems: list[str] = []
    base = "/" + base.strip("/") if base.strip("/") else ""

    for page in sorted(dist.rglob("*.html")):
        text = page.read_text(errors="ignore")
        ids = set(re.findall(r'id="([^"]+)"', text))
        ids |= set(re.findall(r'name="([^"]+)"', text))
        where = page.relative_to(dist)

        for href in sorted(set(re.findall(r'href="([^"]+)"', text))):
            if href.startswith(EXTERNAL) or href == "#":
                continue

            if href.startswith("#"):
                if href[1:] not in ids:
                    problems.append(f"{where}: dead anchor {href}")
                continue

            path = href.split("#", 1)[0].split("?", 1)[0]
            if not path.startswith("/"):
                # Relative links resolve against the page's own directory.
                if not resolves(page.parent, path):
                    problems.append(f"{where}: missing target {href}")
                continue

            if base and not path.startswith(base):
                problems.append(f"{where}: absolute link outside the base -> {href}")
                continue

            if not resolves(dist, path[len(base):].lstrip("/")):
                problems.append(f"{where}: missing target {href}")

    return problems


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dist", type=pathlib.Path)
    ap.add_argument("--base", default="", help="path the site is served under")
    args = ap.parse_args()

    if not args.dist.is_dir():
        print(f"{args.dist} is not a directory -- did the build run?", file=sys.stderr)
        return 1

    pages = list(args.dist.rglob("*.html"))
    if not pages:
        print(f"no HTML under {args.dist}", file=sys.stderr)
        return 1

    problems = check(args.dist, args.base)
    for p in problems:
        print(p, file=sys.stderr)
    print(f"checked {len(pages)} pages, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
