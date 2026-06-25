#!/usr/bin/env python3
"""
Benchmark regression detection script.

This script is intended to be run in CI (e.g., GitHub Actions) after the
benchmark suite has produced a JSON file with the current PR results.

It performs the following steps:
1. Load the current benchmark results (JSON) from the path given by the
   `CURRENT_RESULTS` environment variable.
2. Retrieve historical benchmark data from a time‑series database.
   For the purpose of this repository we use a simple CSV file stored in
   the `benchmark_data/` directory.  In a real CI environment this could be
   replaced with a call to InfluxDB, Prometheus, or any other TSDB.
3. Compute the median baseline for each metric (prove_time_ms,
   verify_gas, mpc_latency_ms) over the last N runs (default 10).
4. Compare the current results against the baseline.  If any metric has
   regressed by more than 10 % a non‑zero exit code is returned and a
   comment payload is written to `COMMENT_PAYLOAD` (JSON) so that the CI
   job can post a comment on the PR.
5. The script exits with code 0 on success (no regression) or 1 on
   regression.

The script is deliberately self‑contained and has no external
dependencies beyond the Python standard library.
"""

import json
import os
import sys
import csv
import statistics
from pathlib import Path
from typing import Dict, List, Tuple

# --------------------------------------------------------------------------- #
# Configuration
# --------------------------------------------------------------------------- #
BASELINE_WINDOW = int(os.getenv("BASELINE_WINDOW", "10"))
REGRESSION_THRESHOLD = float(os.getenv("REGRESSION_THRESHOLD", "0.10"))
HISTORICAL_DIR = Path(os.getenv("HISTORICAL_DIR", "benchmark_data"))
COMMENT_PAYLOAD = Path(os.getenv("COMMENT_PAYLOAD", "benchmark_comment.json"))


def load_current_results() -> Dict[str, float]:
    """
    Load the current benchmark results.

    Expected format (JSON):
    {
        "prove_time_ms": 123.4,
        "verify_gas": 45678,
        "mpc_latency_ms": 78.9
    }
    """
    path = os.getenv("CURRENT_RESULTS")
    if not path:
        print("ERROR: CURRENT_RESULTS environment variable not set", file=sys.stderr)
        sys.exit(1)

    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)

    required = {"prove_time_ms", "verify_gas", "mpc_latency_ms"}
    missing = required - data.keys()
    if missing:
        print(f"ERROR: Missing benchmark keys: {missing}", file=sys.stderr)
        sys.exit(1)

    return {k: float(v) for k, v in data.items()}


def load_historical() -> List[Dict[str, float]]:
    """
    Load historical benchmark data from CSV files in HISTORICAL_DIR.

    Each CSV file should have a header row with the same keys as the JSON
    output of the benchmark suite.
    """
    records: List[Dict[str, float]] = []
    if not HISTORICAL_DIR.is_dir():
        return records

    for csv_file in HISTORICAL_DIR.glob("*.csv"):
        with open(csv_file, newline="", encoding="utf-8") as f:
            reader = csv.DictReader(f)
            for row in reader:
                try:
                    records.append(
                        {
                            "prove_time_ms": float(row["prove_time_ms"]),
                            "verify_gas": float(row["verify_gas"]),
                            "mpc_latency_ms": float(row["mpc_latency_ms"]),
                        }
                    )
                except (KeyError, ValueError):
                    continue
    return records


def compute_baseline(
    records: List[Dict[str, float]], window: int = BASELINE_WINDOW
) -> Dict[str, float]:
    """
    Compute the median baseline for each metric over the most recent `window`
    records.  If there are fewer than `window` records, use all available.
    """
    if not records:
        return {}

    if "timestamp" in records[0]:
        records = sorted(records, key=lambda r: r["timestamp"])

    recent = records[-window:]

    baseline = {}
    for metric in ("prove_time_ms", "verify_gas", "mpc_latency_ms"):
        values = [r[metric] for r in recent if metric in r]
        if values:
            baseline[metric] = statistics.median(values)
    return baseline


def detect_regression(
    current: Dict[str, float], baseline: Dict[str, float]
) -> Tuple[bool, Dict[str, float]]:
    """
    Compare current results with baseline.  Returns a tuple:
    (has_regression, {metric: percent_change, ...})
    """
    regressions: Dict[str, float] = {}
    for metric, cur_val in current.items():
        base_val = baseline.get(metric)
        if base_val is None or base_val == 0:
            continue
        change = (cur_val - base_val) / base_val
        if change > REGRESSION_THRESHOLD:
            regressions[metric] = change
    return (len(regressions) > 0, regressions)


def write_comment_payload(
    regressions: Dict[str, float], baseline: Dict[str, float], current: Dict[str, float]
):
    """
    Write a JSON payload that CI can use to post a comment on the PR.
    """
    lines = ["## 📉 Benchmark Regression Detected\n"]
    lines.append("| Metric | Current | Baseline | Change |\n")
    lines.append("|--------|---------|----------|--------|\n")
    for metric, change in regressions.items():
        cur = f"{current[metric]:.2f}"
        base = f"{baseline[metric]:.2f}"
        pct = f"{change * 100:.1f}%"
        lines.append(f"| `{metric}` | {cur} | {base} | **+{pct}** |\n")

    payload = {"body": "\n".join(lines)}
    COMMENT_PAYLOAD.write_text(json.dumps(payload), encoding="utf-8")


def append_to_historical(current: Dict[str, float]):
    """
    Append the current results to a CSV file for future runs.
    """
    HISTORICAL_DIR.mkdir(parents=True, exist_ok=True)
    csv_path = HISTORICAL_DIR / "benchmarks.csv"
    file_exists = csv_path.is_file()
    with open(csv_path, "a", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(
            f,
            fieldnames=["timestamp", "prove_time_ms", "verify_gas", "mpc_latency_ms"],
        )
        if not file_exists:
            writer.writeheader()
        writer.writerow(
            {
                "timestamp": int(os.getenv("GITHUB_RUN_ID", "0")),
                "prove_time_ms": current["prove_time_ms"],
                "verify_gas": current["verify_gas"],
                "mpc_latency_ms": current["mpc_latency_ms"],
            }
        )


def main():
    current = load_current_results()
    historical = load_historical()
    baseline = compute_baseline(historical)

    has_regression, regressions = detect_regression(current, baseline)

    append_to_historical(current)

    if has_regression:
        write_comment_payload(regressions, baseline, current)
        print("Benchmark regression detected.", file=sys.stderr)
        sys.exit(1)

    print("No benchmark regression detected.")
    sys.exit(0)


if __name__ == "__main__":
    main()
