#!/usr/bin/env python3
"""Check stellar-zk-cards hand-evaluator benchmarks for regressions (issue #652).

Criterion writes one estimates file per benchmark under
``target/criterion/<group>/<bench>/new/estimates.json``. This script reads those
files, compares each benchmark's median against a committed baseline, and fails
when a median grows by more than ``REGRESSION_THRESHOLD`` (10% by default).

It also appends every run to a CSV history file, so the repository keeps a
readable record of how the evaluator has performed over time instead of relying
on a single mutable baseline. On ``main`` the workflow commits that history; on
pull requests it is uploaded as an artifact.

Usage:
    cargo bench -p stellar-zk-cards --bench hand_eval -- --noplot
    python3 scripts/check_hand_evaluator_bench.py

    # Establish or refresh the committed baseline.
    python3 scripts/check_hand_evaluator_bench.py --update-baseline

Environment variables mirror the CLI flags: ``CRITERION_DIR``,
``BASELINE_FILE``, ``HISTORY_FILE``, ``COMMENT_PAYLOAD`` and
``REGRESSION_THRESHOLD``.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import os
import sys
from pathlib import Path
from typing import Any, Dict

DEFAULT_THRESHOLD = float(os.getenv("REGRESSION_THRESHOLD", "0.10"))
HISTORY_FIELDS = [
    "timestamp",
    "git_sha",
    "benchmark",
    "median_ns",
    "mean_ns",
    "std_dev_ns",
    "samples",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--criterion-dir",
        default=os.getenv("CRITERION_DIR", "target/criterion"),
        help="Criterion output directory (default: target/criterion)",
    )
    parser.add_argument(
        "--baseline-file",
        default=os.getenv(
            "BASELINE_FILE", "benchmark_data/hand_evaluator_baseline.json"
        ),
        help="Committed baseline JSON file",
    )
    parser.add_argument(
        "--history-file",
        default=os.getenv(
            "HISTORY_FILE", "benchmark_data/hand_evaluator_history.csv"
        ),
        help="CSV file to append this run to",
    )
    parser.add_argument(
        "--comment-payload",
        default=os.getenv(
            "COMMENT_PAYLOAD", "hand_evaluator_bench_comment.json"
        ),
        help="Where to write the PR comment payload on regression",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=DEFAULT_THRESHOLD,
        help="Maximum allowed relative regression (default: 0.10)",
    )
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="Replace all baseline entries with this run's measurements",
    )
    parser.add_argument(
        "--update-missing-baseline",
        action="store_true",
        help="Add baseline entries for benchmarks that do not have one yet",
    )
    parser.add_argument(
        "--record-history",
        action="store_true",
        help="Append this run to the history CSV",
    )
    parser.add_argument(
        "--git-sha",
        default=os.getenv("GITHUB_SHA", ""),
        help="Commit SHA recorded in the history CSV",
    )
    return parser.parse_args()


def load_current(criterion_dir: Path) -> Dict[str, Dict[str, float]]:
    """Read every Criterion ``new/estimates.json`` under ``criterion_dir``."""
    if not criterion_dir.is_dir():
        raise SystemExit(
            f"ERROR: Criterion directory not found: {criterion_dir}. "
            "Run `cargo bench -p stellar-zk-cards --bench hand_eval` first."
        )

    results: Dict[str, Dict[str, float]] = {}
    for estimates in sorted(criterion_dir.glob("**/new/estimates.json")):
        relative = estimates.relative_to(criterion_dir)
        # target/criterion/<group>/<bench>/new/estimates.json
        parts = relative.parts[:-2]
        if not parts or parts[0] == "report":
            continue
        name = "/".join(parts)

        try:
            data = json.loads(estimates.read_text(encoding="utf-8"))
            results[name] = {
                "median_ns": float(data["median"]["point_estimate"]),
                "mean_ns": float(data["mean"]["point_estimate"]),
                "std_dev_ns": float(data["std_dev"]["point_estimate"]),
            }
        except (KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
            print(
                f"WARNING: ignoring unreadable Criterion result {estimates}: {exc}",
                file=sys.stderr,
            )

    if not results:
        raise SystemExit(
            f"ERROR: no Criterion estimates found under {criterion_dir}. "
            "Did the benchmark run complete?"
        )
    return results


def load_baseline(path: Path) -> Dict[str, Dict[str, float]]:
    if not path.is_file():
        return {}
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"ERROR: invalid baseline JSON {path}: {exc}") from exc

    entries = raw.get("benchmarks", raw) if isinstance(raw, dict) else {}
    baseline: Dict[str, Dict[str, float]] = {}
    if not isinstance(entries, dict):
        raise SystemExit(f"ERROR: baseline {path} has no 'benchmarks' object")

    for name, value in entries.items():
        if isinstance(value, (int, float)):
            baseline[name] = {"median_ns": float(value)}
        elif isinstance(value, dict) and "median_ns" in value:
            baseline[name] = {"median_ns": float(value["median_ns"])}
        else:
            print(
                f"WARNING: skipping malformed baseline entry {name!r}",
                file=sys.stderr,
            )
    return baseline


def write_baseline(
    path: Path,
    current: Dict[str, Dict[str, float]],
    existing: Dict[str, Dict[str, float]],
    update_missing_only: bool,
) -> None:
    entries: Dict[str, Any] = {}
    if update_missing_only:
        entries.update(
            {
                name: {"median_ns": value["median_ns"]}
                for name, value in existing.items()
            }
        )
    for name, value in current.items():
        if update_missing_only and name in entries:
            continue
        entries[name] = {
            "median_ns": round(value["median_ns"], 3),
            "mean_ns": round(value["mean_ns"], 3),
        }

    payload = {
        "schema_version": 1,
        "metric": "median_ns",
        "benchmarks": dict(sorted(entries.items())),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def append_history(
    path: Path,
    current: Dict[str, Dict[str, float]],
    git_sha: str,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    file_exists = path.is_file() and path.stat().st_size > 0
    timestamp = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
    with path.open("a", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=HISTORY_FIELDS)
        if not file_exists:
            writer.writeheader()
        for name, value in sorted(current.items()):
            writer.writerow(
                {
                    "timestamp": timestamp,
                    "git_sha": git_sha,
                    "benchmark": name,
                    "median_ns": f"{value['median_ns']:.3f}",
                    "mean_ns": f"{value['mean_ns']:.3f}",
                    "std_dev_ns": f"{value['std_dev_ns']:.3f}",
                    "samples": "",
                }
            )


def format_report(
    regressions: Dict[str, float],
    current: Dict[str, Dict[str, float]],
    baseline: Dict[str, Dict[str, float]],
    threshold: float,
) -> str:
    lines = [
        "## Hand evaluator benchmark regression",
        "",
        f"A `stellar-zk-cards` benchmark regressed by more than "
        f"**{threshold * 100:.1f}%** versus "
        "`benchmark_data/hand_evaluator_baseline.json`.",
        "",
        "| Benchmark | Baseline (ns) | Current (ns) | Change |",
        "|---|---:|---:|---:|",
    ]
    for name, change in sorted(regressions.items()):
        base = baseline[name]["median_ns"]
        cur = current[name]["median_ns"]
        lines.append(
            f"| `{name}` | {base:,.1f} | {cur:,.1f} | **+{change * 100:.1f}%** |"
        )
    lines.extend(
        [
            "",
            "Re-run locally with:",
            "",
            "```bash",
            "cargo bench -p stellar-zk-cards --bench hand_eval -- --noplot",
            "python3 scripts/check_hand_evaluator_bench.py",
            "```",
            "",
            "If the change is intentional, refresh the committed baseline with "
            "`python3 scripts/check_hand_evaluator_bench.py --update-baseline` "
            "and include the new numbers in the PR description.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    current = load_current(Path(args.criterion_dir))
    baseline_path = Path(args.baseline_file)
    baseline = load_baseline(baseline_path)

    print(f"Measured {len(current)} benchmarks:")
    for name, value in sorted(current.items()):
        base = baseline.get(name, {}).get("median_ns")
        suffix = f" (baseline {base:,.1f} ns)" if base is not None else " (no baseline)"
        print(f"  {name:<32} {value['median_ns']:>12,.1f} ns{suffix}")

    missing_current = sorted(set(baseline) - set(current))
    if missing_current:
        print(
            "ERROR: baseline contains benchmarks that did not run: "
            + ", ".join(missing_current)
            + ". Restore the benchmark or update the baseline explicitly.",
            file=sys.stderr,
        )
        return 2

    regressions: Dict[str, float] = {}
    for name, value in current.items():
        base = baseline.get(name, {}).get("median_ns")
        if base is None or base <= 0:
            continue
        change = (value["median_ns"] - base) / base
        if change > args.threshold:
            regressions[name] = change

    if args.update_baseline or args.update_missing_baseline:
        write_baseline(
            baseline_path,
            current,
            baseline,
            update_missing_only=not args.update_baseline,
        )
        print(f"Updated baseline: {baseline_path}")

    if args.record_history:
        append_history(Path(args.history_file), current, args.git_sha)
        print(f"Recorded benchmark history: {args.history_file}")

    if regressions:
        report = format_report(regressions, current, baseline, args.threshold)
        if args.comment_payload:
            payload_path = Path(args.comment_payload)
            payload_path.parent.mkdir(parents=True, exist_ok=True)
            payload_path.write_text(
                json.dumps({"body": report}, indent=2) + "\n", encoding="utf-8"
            )
        print(report, file=sys.stderr)
        print(
            f"ERROR: {len(regressions)} benchmark(s) regressed by more than "
            f"{args.threshold * 100:.1f}%.",
            file=sys.stderr,
        )
        return 1

    if not baseline:
        print(
            "No baseline entries yet; this run passed. The main-branch workflow "
            "will record them as the initial baseline."
        )
    else:
        print(
            f"No regression above {args.threshold * 100:.1f}% "
            f"({len(current)} benchmarks checked)."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
