#!/usr/bin/env python3
"""Fail if a credential has been committed.

GitHub's own secret scanning covers the token formats it knows about, but the
thing this project is most likely to leak is not one of them: a YouTube Music
`Cookie` header, which is just a long string of ordinary cookies and
authenticates as the account holder. So look for that specifically, in the
working tree and in the history.

Run locally with:

    python3 .github/scripts/check_secrets.py
"""

import pathlib
import re
import subprocess
import sys

# Files that must never be tracked, whatever they contain.
FORBIDDEN_NAMES = {"cookie.txt", "oauth.json"}

# A YouTube auth cookie *with a value*. The bare names appear legitimately --
# src/browser.rs lists the ones to collect -- so the value is what makes it a
# credential rather than a mention.
CREDENTIAL = re.compile(
    r"(?:SAPISID|__Secure-1PSID|__Secure-3PSID|__Secure-1PAPISID"
    r"|__Secure-3PAPISID|HSID|SSID|APISID)=[A-Za-z0-9_\-/+]{16,}"
)

# Text files only: a match inside a PNG is noise.
SKIP_SUFFIXES = {".png", ".jpg", ".jpeg", ".webp", ".ico", ".svg", ".lock"}


def run(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, check=False).stdout


def tracked_files() -> list[pathlib.Path]:
    out = run("git", "ls-files", "-z")
    return [pathlib.Path(p) for p in out.split("\0") if p]


def main() -> int:
    problems: list[str] = []

    for path in tracked_files():
        if path.name in FORBIDDEN_NAMES:
            problems.append(f"{path}: credential file is tracked")
            continue
        if path.suffix.lower() in SKIP_SUFFIXES:
            continue
        try:
            text = path.read_text(errors="ignore")
        except OSError:
            continue
        for match in CREDENTIAL.finditer(text):
            name = match.group(0).split("=", 1)[0]
            line = text[: match.start()].count("\n") + 1
            problems.append(f"{path}:{line}: looks like a live {name} cookie")

    # The history matters as much as the tip: rewriting a file does not
    # unpublish what was pushed.
    history = run("git", "log", "--all", "--diff-filter=A", "--name-only",
                  "--pretty=format:", "--")
    for name in {n.strip() for n in history.splitlines() if n.strip()}:
        if pathlib.PurePath(name).name in FORBIDDEN_NAMES:
            problems.append(f"{name}: credential file was committed at some point in history")

    if problems:
        print("Credential check failed:\n", file=sys.stderr)
        for p in sorted(set(problems)):
            print(f"  {p}", file=sys.stderr)
        print(
            "\nIf this is a real credential, rotate it first: signing out of "
            "Google everywhere revokes a leaked cookie header.",
            file=sys.stderr,
        )
        return 1

    print(f"credential check ok ({len(tracked_files())} tracked files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
