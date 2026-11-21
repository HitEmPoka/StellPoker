#!/usr/bin/env python3
"""Check that every local link in Markdown files resolves to an existing file.

Walks all *.md files, extracts `[text](url)` link targets, and verifies that
local (non-http, non-mailto, non-anchor-only) targets exist on disk relative
to the document containing them. Exits non-zero on the first broken link.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")

ROOT = Path(__file__).resolve().parent.parent


def is_local_link(target: str) -> bool:
    """True if `target` is a local file link we should verify."""
    if target.startswith(("http://", "https://", "mailto:", "tel:")):
        return False
    # Anchor-only links (e.g. `#section`) point within the same document.
    if target.startswith("#") or target == "":
        return False
    # Fragment without a path (e.g. `file.md#section`) still references file.md.
    return True


def link_target(target: str) -> str:
    """Return the path portion of a link target, stripping fragment/query."""
    return target.split("#", 1)[0].split("?", 1)[0]


def main() -> int:
    broken: list[tuple[str, str, str]] = []  # (doc, target, resolved)
    md_files = sorted(ROOT.rglob("*.md"))

    for doc in md_files:
        try:
            text = doc.read_text(encoding="utf-8")
        except OSError:
            continue
        doc_dir = doc.parent
        for match in LINK_RE.finditer(text):
            target = match.group(1)
            if not is_local_link(target):
                continue
            resolved = (doc_dir / link_target(target)).resolve()
            # Stay within the repository; links escaping the root are invalid.
            try:
                resolved.relative_to(ROOT)
            except ValueError:
                broken.append((str(doc), target, str(resolved)))
                continue
            if not resolved.exists():
                broken.append((str(doc), target, str(resolved)))

    if broken:
        print("Broken local links found:")
        for doc, target, resolved in broken:
            print(f"  {doc}: {target} -> {resolved} (missing)")
        return 1

    print(f"All local links resolved across {len(md_files)} markdown files.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
