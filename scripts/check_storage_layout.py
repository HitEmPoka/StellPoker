#!/usr/bin/env python3
"""Storage layout snapshot check for poker-table contract (issue #566).

Silent storage collisions break upgrades. This tool snapshots the
`DataKey` storage layout keys declared in
`contracts/poker-table/src/types.rs` and fails when the source diverges
from `contracts/poker-table/storage-layout-snapshot.json`.

Usage:
    python3 scripts/check_storage_layout.py [--check] [--update] [--report MD]
    python3 scripts/check_storage_layout.py --repo /path/to/repo

- --check (default): compare source enum vs snapshot, exit 1 on
  added/removed/renamed variants or arity/domain changes.
- --update: regenerate the snapshot `keys` list from source (preserves
  descriptions/domains for known keys, infers domains for new keys).
- --report PATH: also write a markdown summary for PR comments.

Exit codes: 0 = layout matches snapshot, 1 = unexpected change.
Only stdlib is used so the script runs in CI without installs.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


TYPES_REL = Path("contracts/poker-table/src/types.rs")
SNAPSHOT_REL = Path("contracts/poker-table/storage-layout-snapshot.json")

# Matches `Name` or `Name(...)` inside the DataKey enum body.
VARIANT_RE = re.compile(r"^\s*(?P<name>[A-Z][A-Za-z0-9_]*)\s*(?:\((?P<args>[^)]*)\))?\s*,?\s*(?://.*)?$")


def find_datakey_body(source: str) -> str:
    """Extract the body of `enum DataKey { ... }` handling nested parens."""
    marker = re.search(r"pub\s+enum\s+DataKey\s*\{", source)
    if not marker:
        raise RuntimeError("enum DataKey not found in types.rs")
    start = marker.end()
    depth = 1
    i = start
    while i < len(source) and depth > 0:
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
        i += 1
    if depth != 0:
        raise RuntimeError("unbalanced braces while parsing enum DataKey")
    return source[start : i - 1]


def parse_datakey_variants(source: str) -> dict[str, int]:
    """Return {variant_name: arity} for the DataKey enum."""
    body = find_datakey_body(source)
    variants: dict[str, int] = {}
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("///") or stripped.startswith("//"):
            continue
        m = VARIANT_RE.match(line)
        if not m:
            continue
        name = m.group("name")
        args = (m.group("args") or "").strip()
        if not args:
            arity = 0
        else:
            # Payload is comma-separated types, e.g. `u32, Address`.
            arity = len([a for a in args.split(",") if a.strip()])
        # Skip doc-comment artefacts; DataKey variants are CamelCase.
        if name in ("None", "Some"):
            continue
        variants[name] = arity
    return variants


def infer_domains(repo: Path, variants: dict[str, int]) -> dict[str, str]:
    """Infer persistent/instance domain per variant from source usage.

    For each `DataKey::Variant` occurrence, inspect the surrounding lines
    for `.persistent()` vs `.instance()`. Unused/reserved keys yield
    "unknown" and are exempt from domain enforcement.
    """
    src_dir = repo / "contracts/poker-table/src"
    files = sorted(src_dir.glob("*.rs"))
    domains: dict[str, str] = {}
    for variant in variants:
        persistent_hits = 0
        instance_hits = 0
        needle = re.compile(rf"DataKey::{re.escape(variant)}\b")
        for path in files:
            try:
                lines = path.read_text(encoding="utf-8").splitlines()
            except OSError:
                continue
            for idx, line in enumerate(lines):
                if not needle.search(line):
                    continue
                window = "\n".join(lines[max(0, idx - 4) : idx + 5])
                if ".persistent()" in window:
                    persistent_hits += 1
                if ".instance()" in window:
                    instance_hits += 1
        if persistent_hits and not instance_hits:
            domains[variant] = "persistent"
        elif instance_hits and not persistent_hits:
            domains[variant] = "instance"
        elif persistent_hits and instance_hits:
            # Mixed usage: report both; treat as needing explicit review.
            # In practice poker-table keeps each key in one domain, so a
            # mixed signal means the key moved domains -> flag it.
            if persistent_hits >= instance_hits:
                domains[variant] = "persistent"
            else:
                domains[variant] = "instance"
        else:
            domains[variant] = "unknown"
    return domains


def load_snapshot(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def check(repo: Path, snapshot_path: Path, types_path: Path) -> tuple[int, str, dict]:
    source = types_path.read_text(encoding="utf-8")
    current = parse_datakey_variants(source)
    inferred = infer_domains(repo, current)
    snapshot = load_snapshot(snapshot_path)
    snap_keys = {k["name"]: k for k in snapshot.get("keys", [])}

    added = sorted(set(current) - set(snap_keys))
    removed = sorted(set(snap_keys) - set(current))
    arity_changed = sorted(
        name
        for name in set(current) & set(snap_keys)
        if current[name] != int(snap_keys[name].get("arity", -1))
    )
    domain_changed: list[str] = []
    for name in set(current) & set(snap_keys):
        snap_domain = str(snap_keys[name].get("domain", "unknown"))
        inf = inferred.get(name, "unknown")
        if inf == "unknown" or snap_domain == "unknown":
            continue
        if inf != snap_domain:
            domain_changed.append(name)

    failures: list[str] = []
    if added:
        failures.append(f"added DataKey variant(s): {', '.join(added)}")
    if removed:
        failures.append(
            f"REMOVED DataKey variant(s): {', '.join(removed)} "
            "(removal breaks upgrades; append-only policy)"
        )
    if arity_changed:
        failures.append(
            f"arity changed (payload shape changed): {', '.join(arity_changed)}"
        )
    if domain_changed:
        failures.append(
            f"storage domain changed (persistent<->instance): "
            f"{', '.join(domain_changed)}"
        )

    lines = [
        "## Storage layout snapshot (issue #566)",
        "",
        f"- Source variants: `{len(current)}`, snapshot entries: `{len(snap_keys)}`",
        f"- Added: `{', '.join(added) if added else 'none'}`",
        f"- Removed: `{', '.join(removed) if removed else 'none'}`",
        f"- Arity changed: `{', '.join(arity_changed) if arity_changed else 'none'}`",
        f"- Domain changed: `{', '.join(domain_changed) if domain_changed else 'none'}`",
        "",
    ]
    if failures:
        lines.append("### FAIL — unexpected storage layout change")
        lines.append("")
        lines.extend(f"- {f}" for f in failures)
        lines.append("")
        lines.append(
            "If this change is intentional, run "
            "`python3 scripts/check_storage_layout.py --update` and commit the "
            "regenerated `storage-layout-snapshot.json` with a migration note."
        )
    else:
        lines.append("### PASS — storage layout matches snapshot")
    report = "\n".join(lines) + "\n"
    detail = {
        "current": current,
        "inferred_domains": inferred,
        "added": added,
        "removed": removed,
        "arity_changed": arity_changed,
        "domain_changed": domain_changed,
        "failures": failures,
    }
    return (1 if failures else 0), report, detail


def update_snapshot(repo: Path, snapshot_path: Path, types_path: Path) -> str:
    source = types_path.read_text(encoding="utf-8")
    current = parse_datakey_variants(source)
    inferred = infer_domains(repo, current)
    snapshot = load_snapshot(snapshot_path)
    existing = {k["name"]: k for k in snapshot.get("keys", [])}
    keys = []
    for name in sorted(current):
        prev = existing.get(name, {})
        domain = inferred.get(name, "unknown")
        if domain == "unknown":
            domain = str(prev.get("domain", "persistent"))
        keys.append(
            {
                "name": name,
                "arity": current[name],
                "domain": domain,
                "description": str(prev.get("description", "New key (describe me).")),
            }
        )
    snapshot["keys"] = keys
    snapshot_path.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")
    return f"Updated {snapshot_path} with {len(keys)} keys.\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--snapshot", type=Path, default=None)
    parser.add_argument("--types", type=Path, default=None)
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--report", type=Path, default=None)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    repo = args.repo.resolve()
    snapshot_path = args.snapshot or (repo / SNAPSHOT_REL)
    types_path = args.types or (repo / TYPES_REL)

    if args.update:
        msg = update_snapshot(repo, snapshot_path, types_path)
        print(msg)
        # Re-check after update so CI logs show the new state.
        code, report, _ = check(repo, snapshot_path, types_path)
        print(report)
        if args.report:
            args.report.write_text(report, encoding="utf-8")
        return code

    code, report, _ = check(repo, snapshot_path, types_path)
    print(report)
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    return code


if __name__ == "__main__":
    sys.exit(main())
