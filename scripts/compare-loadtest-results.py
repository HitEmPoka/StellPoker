#!/usr/bin/env python3
"""Compare a current k6 load-test summary against a baseline and flag regressions.

k6 writes a `summary.json` when invoked with `--summary-export=…`. This script
reads two of them (baseline and current), compares the metrics we care about,
and fails if any metric regresses beyond the configured threshold.

Usage:
    compare-loadtest-results.py --baseline baseline.json --current current.json \
                                [--threshold-pct 10] [--out report.md]

Exit codes:
    0  — within thresholds
    1  — regression detected
    2  — usage / parse error
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# metric_path (dotted) -> (direction, label)
# direction: "lower-is-better" or "higher-is-better"
WATCHED_METRICS = {
    "metrics.http_req_duration.values.p(95)":   ("lower-is-better",  "HTTP p95 latency (ms)"),
    "metrics.http_req_duration.values.p(99)":   ("lower-is-better",  "HTTP p99 latency (ms)"),
    "metrics.http_req_duration.values.avg":     ("lower-is-better",  "HTTP avg latency (ms)"),
    "metrics.http_req_failed.values.rate":      ("lower-is-better",  "HTTP failure rate"),
    "metrics.errors.values.rate":               ("lower-is-better",  "Custom error rate"),
    "metrics.response_time_ms.values.p(99)":    ("lower-is-better",  "Custom p99 response time (ms)"),
    "metrics.http_reqs.values.rate":            ("higher-is-better", "HTTP requests/sec"),
    "metrics.iterations.values.rate":           ("higher-is-better", "Iterations/sec"),
}


def dig(obj: dict, path: str):
    cur = obj
    for part in path.split("."):
        if not isinstance(cur, dict) or part not in cur:
            return None
        cur = cur[part]
    return cur


def pct_change(baseline: float, current: float) -> float:
    if baseline == 0:
        return float("inf") if current != 0 else 0.0
    return (current - baseline) / baseline * 100.0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline", required=True, type=Path, help="Baseline summary.json")
    ap.add_argument("--current", required=True, type=Path, help="Current summary.json")
    ap.add_argument(
        "--threshold-pct", type=float, default=10.0,
        help="Fail if any watched metric regresses by more than this percent (default: 10)",
    )
    ap.add_argument("--out", type=Path, help="Write a Markdown report to this path")
    args = ap.parse_args()

    for p in (args.baseline, args.current):
        if not p.is_file():
            print(f"ERROR: {p} does not exist", file=sys.stderr)
            return 2

    try:
        baseline = json.loads(args.baseline.read_text())
        current = json.loads(args.current.read_text())
    except json.JSONDecodeError as e:
        print(f"ERROR: failed to parse JSON — {e}", file=sys.stderr)
        return 2

    rows: list[tuple[str, float | None, float | None, float | None, bool]] = []
    regressed = False

    for path, (direction, label) in WATCHED_METRICS.items():
        b = dig(baseline, path)
        c = dig(current, path)
        if b is None or c is None:
            rows.append((label, b, c, None, False))
            continue
        change = pct_change(b, c)
        # Lower-is-better: positive change is bad.
        # Higher-is-better: negative change is bad.
        if direction == "lower-is-better":
            bad = change > args.threshold_pct
        else:
            bad = change < -args.threshold_pct
        if bad:
            regressed = True
        rows.append((label, float(b), float(c), change, bad))

    # Markdown report.
    lines: list[str] = []
    lines.append(f"# Load-test regression report\n")
    lines.append(f"- Baseline: `{args.baseline.name}`")
    lines.append(f"- Current:  `{args.current.name}`")
    lines.append(f"- Threshold: ±{args.threshold_pct:.1f}%\n")
    lines.append("| Metric | Baseline | Current | Change | Status |")
    lines.append("|---|---:|---:|---:|:-:|")
    for label, b, c, change, bad in rows:
        if b is None or c is None:
            lines.append(f"| {label} | — | — | — | ⚠ missing |")
            continue
        marker = "❌" if bad else "✅"
        lines.append(f"| {label} | {b:.3f} | {c:.3f} | {change:+.1f}% | {marker} |")
    verdict = "REGRESSION" if regressed else "within thresholds"
    lines.append(f"\n**Verdict:** {verdict}\n")

    report = "\n".join(lines)
    print(report)
    if args.out:
        args.out.write_text(report)

    return 1 if regressed else 0


if __name__ == "__main__":
    sys.exit(main())
