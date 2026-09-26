#!/usr/bin/env python3
"""Soroban budget ceiling check with PR reporting (issue #567).

#305 gas regression: define hard ceilings per entrypoint. CI fails if any
function exceeds its CPU/memory budget ceiling.

This script is the static half of the suite (no toolchain needed):
- loads `contracts/poker-table/ceilings.json` (hard ceilings)
- loads `contracts/gas-budgets.json` (approved baselines)
- fails when a ceiling is missing, malformed, or *below* its baseline
  (a ceiling below baseline would fail on every run)
- writes a markdown report for PR comments (`--report`) and step summary

The dynamic half runs in CI via `cargo test -p poker-table budget_ceiling`
(see `contracts/poker-table/src/budget_ceilings_test.rs`), which measures
live CPU/memory per entrypoint and asserts it stays under these ceilings.

Usage:
    python3 scripts/check_soroban_budgets.py [--check] [--report REPORT.md]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


CEILINGS_REL = Path("contracts/poker-table/ceilings.json")
BASELINE_REL = Path("contracts/gas-budgets.json")


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def check(repo: Path, ceilings_path: Path, baseline_path: Path) -> tuple[int, str, dict]:
    ceilings = load_json(ceilings_path)
    baseline = load_json(baseline_path) if baseline_path.exists() else {"functions": {}}

    ceil_fns: dict = ceilings.get("functions", {})
    base_fns: dict = baseline.get("functions", {})

    failures: list[str] = []
    rows: list[tuple[str, int, int, int, str]] = []

    for name in sorted(set(ceil_fns) | set(base_fns)):
        ceil = ceil_fns.get(name)
        base = base_fns.get(name)
        if ceil is None:
            failures.append(f"`{name}` missing from ceilings.json (add a hard ceiling)")
            rows.append((name, -1, -1, base.get("cpu_insns", -1) if base else -1, "MISSING"))
            continue
        try:
            cpu_ceiling = int(ceil["cpu_ceiling"])
            mem_ceiling = int(ceil["mem_ceiling"])
        except (KeyError, TypeError, ValueError):
            failures.append(f"`{name}` must define integer cpu_ceiling + mem_ceiling")
            rows.append((name, -1, -1, -1, "MALFORMED"))
            continue
        if cpu_ceiling <= 0 or mem_ceiling <= 0:
            failures.append(f"`{name}` ceilings must be positive")
        base_cpu = int(base.get("cpu_insns", 0)) if base else 0
        status = "OK"
        if base and cpu_ceiling < base_cpu:
            failures.append(
                f"`{name}` cpu_ceiling {cpu_ceiling} is below baseline {base_cpu} "
                f"(would fail every run; raise ceiling or lower cost)"
            )
            status = "CEILING<BASELINE"
        rows.append((name, cpu_ceiling, mem_ceiling, base_cpu, status))

    # ceilings.json must not silently drop a baselined function
    for name in sorted(set(base_fns) - set(ceil_fns)):
        if name not in failures:
            pass  # already reported as missing above

    lines = [
        "## Soroban budget ceilings (issue #567)",
        "",
        "| Function | CPU ceiling (insns) | Mem ceiling (bytes) | Baseline CPU | Status |",
        "|---|---:|---:|---:|---|",
    ]
    for name, cpu_c, mem_c, base_cpu, status in rows:
        mark = "✅" if status == "OK" else "❌"
        lines.append(
            f"| `{name}` | {cpu_c:,} | {mem_c:,} | {base_cpu:,} | {mark} {status} |"
        )
    lines += [
        "",
        "Live enforcement runs in CI via `cargo test -p poker-table budget_ceiling` "
        "plus the legacy `gas_*` regression tests (5% over baseline fails).",
        "",
    ]
    if failures:
        lines.append("### FAIL — budget definition problem")
        lines.append("")
        lines.extend(f"- {f}" for f in failures)
    else:
        lines.append("### PASS — all ceilings cover their baselines")
    report = "\n".join(lines) + "\n"
    detail = {"failures": failures, "rows": rows}
    return (1 if failures else 0), report, detail


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--ceilings", type=Path, default=None)
    parser.add_argument("--baseline", type=Path, default=None)
    parser.add_argument("--report", type=Path, default=None)
    args = parser.parse_args()

    repo = args.repo.resolve()
    ceilings_path = args.ceilings or (repo / CEILINGS_REL)
    baseline_path = args.baseline or (repo / BASELINE_REL)

    code, report, _ = check(repo, ceilings_path, baseline_path)
    print(report)
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    return code


if __name__ == "__main__":
    sys.exit(main())
