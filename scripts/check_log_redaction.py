#!/usr/bin/env python3
"""Grep-based CI check (Issue #509).

Scans the Rust services (and contracts) for `tracing::{level}!` invocations and
flags any *structured field key* that could carry card values, MPC secret
shares, commitment salts, or credentials.

This is the lint half of the log-redaction policy; the runtime half is the
redacting `MakeWriter` in `services/{coordinator,node}/src/redact.rs`, and the
policy itself is documented in `docs/unified-logging-schema.md`.

KEEP THE FORBIDDEN LIST IN SYNC with `SENSITIVE_FIELD_KEYS` in redact.rs.

Usage:
    python3 scripts/check_log_redaction.py          # scan default roots
    python3 scripts/check_log_redaction.py <paths>  # scan given paths
Exit code 0 = clean, 1 = at least one forbidden log field found.
"""

from __future__ import annotations

import pathlib
import re
import sys

FORBIDDEN = [
    # Card values -- never log hole cards or any resolved card.
    "hole_cards",
    "hole_card",
    "hole_card1",
    "hole_card2",
    "card1",
    "card2",
    "cards",
    "card",
    "player_card_positions",
    # Commitment salts -- reveal the committed value when read with the hand.
    "salts",
    "salt1",
    "salt2",
    "salt",
    # MPC secret shares / share bundles.
    "shares",
    "share",
    "share_bundle",
    "share_set_id",
    "share_ids",
    # Credentials and session secrets.
    "secret",
    "secret_key",
    "private_key",
    "committee_secret",
    "ciphertext",
    "api_key",
    "authorization",
    "password",
]

# `tracing::info!` ... any level, any indent.
MACRO_OPEN = re.compile(r"tracing::\s*(?:trace|debug|info|warn|error)\s*!\s*\(")
# Structured field key inside a macro argument region: `field = value` with an
# optional `?`/`%` sigil. Negative lookahead avoids `==` comparisons.
FIELD_KEY = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:[?%])?(?!=)")


def is_sensitive(field: str) -> bool:
    """Boundary-aware matching identical to redact.rs::is_sensitive_key."""
    f = field.lower()
    for needle in FORBIDDEN:
        if f == needle:
            return True
        if f.startswith(needle + "_") or f.endswith("_" + needle):
            return True
        if ("_" + needle + "_") in f:
            return True
    return False


def macro_blocks(text: str):
    """Yield (start, block) pairs for each complete tracing macro invocation."""
    for m in MACRO_OPEN.finditer(text):
        i = m.end() - 1  # index of '('
        depth = 0
        while i < len(text):
            c = text[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        yield m.start(), text[m.start() : i + 1]


def scan_file(path: pathlib.Path) -> list:
    violations = []
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:  # pragma: no cover
        print(f"!! cannot read {path}: {exc}", file=sys.stderr)
        return violations

    for block_start, block in macro_blocks(text):
        # Line number of the macro opening.
        line_no = text.count("\n", 0, block_start) + 1
        for fm in FIELD_KEY.finditer(block):
            field = fm.group(1)
            if is_sensitive(field):
                violations.append((path, line_no, field, block.strip()))
    return violations


def main(argv) -> int:
    roots = [pathlib.Path(p) for p in (argv[1:] or ["services", "contracts"])]
    files: list[pathlib.Path] = []
    for root in roots:
        if root.is_file():
            files.append(root)
        elif root.is_dir():
            files.extend(sorted(root.rglob("*.rs")))
        else:
            print(f"!! path not found: {root}", file=sys.stderr)

    violations: list = []
    for path in files:
        violations.extend(scan_file(path))

    if violations:
        print(f"Log-redaction check FAILED ({len(violations)} violation(s))", file=sys.stderr)
        for path, line, field, snippet in violations:
            print(f"  {path}:{line} forbidden log field `{field}`", file=sys.stderr)
            print(f"    {snippet[:200]}", file=sys.stderr)
        return 1

    print(f"Log-redaction check passed ({len(files)} file(s) scanned).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))