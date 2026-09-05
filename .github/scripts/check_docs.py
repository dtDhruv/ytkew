#!/usr/bin/env python3
"""Check the documentation site before it is published.

A broken anchor or an unclosed tag is invisible until someone clicks it, so
this fails the build instead. Kept as a script rather than inline YAML so it
can be run locally:

    python3 .github/scripts/check_docs.py
"""

import html.parser
import pathlib
import re
import sys

# Tags that never have a closing partner, so the stack must not track them.
VOID = {
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "source", "track", "wbr",
}


class Structure(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.stack: list[tuple[str, int]] = []
        self.problems: list[str] = []

    def handle_starttag(self, tag: str, attrs: object) -> None:
        if tag not in VOID:
            self.stack.append((tag, self.getpos()[0]))

    def handle_endtag(self, tag: str) -> None:
        if tag in VOID:
            return
        if self.stack and self.stack[-1][0] == tag:
            self.stack.pop()
        else:
            open_tag = self.stack[-1][0] if self.stack else "nothing"
            line = self.getpos()[0]
            self.problems.append(f"line {line}: </{tag}> does not close <{open_tag}>")


def check(page: pathlib.Path) -> list[str]:
    text = page.read_text()
    found: list[str] = []

    parser = Structure()
    parser.feed(text)
    found += parser.problems
    for tag, line in parser.stack:
        found.append(f"line {line}: <{tag}> is never closed")

    ids = set(re.findall(r'id="([^"]+)"', text))
    for target in sorted(set(re.findall(r'href="#([^"]+)"', text))):
        if target not in ids:
            found.append(f'href="#{target}" has no matching id')

    # Local assets have to exist, or the page ships with a broken image.
    for ref in sorted(set(re.findall(r'(?:href|src)="(?!https?:|#|mailto:|data:)([^"]+)"', text))):
        if not (page.parent / ref).exists():
            found.append(f"references missing file {ref}")

    return found


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2] / "docs"
    pages = sorted(root.rglob("*.html"))
    if not pages:
        print(f"no HTML found under {root}", file=sys.stderr)
        return 1

    failed = False
    for page in pages:
        problems = check(page)
        rel = page.relative_to(root.parent)
        if problems:
            failed = True
            for problem in problems:
                print(f"{rel}: {problem}", file=sys.stderr)
        else:
            print(f"{rel}: ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
