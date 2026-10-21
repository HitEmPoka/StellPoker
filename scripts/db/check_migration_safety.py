#!/usr/bin/env python3
"""Static safety check for coordinator migrations.

Flags operations that are unsafe in a blue-green deploy. Pass one or more
migration base names (e.g. 20260615_expand_add_player_region) or shell globs;
the script resolves both the .up.sql and .down.sql halves.

Lines carrying the exact comment `-- blue-green: acknowledged` are skipped.
Use that override only with a written justification and a second reviewer.

Exit codes:
    0 — safe to deploy
    1 — one or more unsafe patterns found
    2 — usage error
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

MIGRATIONS_DIR = Path(__file__).resolve().parent.parent.parent / "services" / "coordinator" / "migrations"

# (regex, human label) — all applied case-insensitively per-line.
PATTERNS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\bDROP\s+COLUMN\b", re.I),                                   "DROP COLUMN (contract-only)"),
    (re.compile(r"\bDROP\s+TABLE\b", re.I),                                    "DROP TABLE (contract-only)"),
    (re.compile(r"\bALTER\s+COLUMN\b.*\bTYPE\b", re.I),                        "ALTER COLUMN ... TYPE (rewrites rows)"),
    (re.compile(r"\bRENAME\s+(COLUMN|TO)\b", re.I),                            "RENAME (never — use expand + contract)"),
    # NOT NULL on an ADD COLUMN is only safe when a DEFAULT is also set — look for
    # the pair anywhere on the same line; flag only when NOT NULL is present and
    # DEFAULT is not.
    (re.compile(r"\bADD\s+COLUMN\b(?=.*\bNOT\s+NULL\b)(?!.*\bDEFAULT\b)", re.I), "ADD COLUMN NOT NULL without DEFAULT"),
    (re.compile(r"\bCREATE\s+(?:UNIQUE\s+)?INDEX\b(?!\s+CONCURRENTLY)", re.I), "CREATE INDEX without CONCURRENTLY"),
    (re.compile(r"\bLOCK\s+TABLE\b", re.I),                                    "explicit LOCK TABLE"),
    (re.compile(r"\bTRUNCATE\b", re.I),                                        "TRUNCATE"),
]

ACKNOWLEDGE_RE = re.compile(r"--\s*blue-green:\s*acknowledged.*$", re.I)


def resolve_files(args: list[str]) -> list[Path]:
    files: list[Path] = []
    for arg in args:
        matches = sorted(MIGRATIONS_DIR.glob(f"{arg}*.sql"))
        files.extend(matches)
    return files


def check_file(path: Path) -> list[tuple[int, str, str]]:
    findings: list[tuple[int, str, str]] = []
    for lineno, raw in enumerate(path.read_text(errors="replace").splitlines(), start=1):
        line = ACKNOWLEDGE_RE.sub("", raw).strip()
        if not line:
            continue
        for regex, label in PATTERNS:
            if regex.search(line):
                findings.append((lineno, label, raw.strip()))
    return findings


def main(argv: list[str]) -> int:
    args = argv[1:]
    if not args:
        print("Usage: check_migration_safety.py <migration_name_or_glob>...", file=sys.stderr)
        return 2

    files = resolve_files(args)
    if not files:
        print(f"ERROR: no migration files matched {args}", file=sys.stderr)
        return 2

    status = 0
    for f in files:
        rel = f.relative_to(MIGRATIONS_DIR.parent.parent.parent)
        for lineno, label, content in check_file(f):
            print(f"unsafe: {rel}:{lineno} — {label}")
            print(f"        {content}")
            status = 1

    if status == 0:
        print(f"safe: {len(files)} file(s) checked, no unsafe patterns found")
    else:
        print(
            "\nSee docs/blue-green-migrations.md for the expand-contract rules.\n"
            "To intentionally override a specific line, append:\n"
            "    -- blue-green: acknowledged (reason, reviewer)",
            file=sys.stderr,
        )
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
