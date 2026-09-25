#!/usr/bin/env python3
"""Collect and enforce Noir circuit constraint budgets.

The script reads `nargo info --json` and `bb gates` output for each circuit,
compares the metrics against configured budgets and an optional baseline run,
then writes JSON/Markdown/SVG artifacts for CI history.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_CIRCUITS = (
    "deal_valid",
    "reveal_board_valid",
    "showdown_valid",
    "muck_valid",
    "burn_card_valid",
    "side_pot_valid",
    "split_pot_valid",
    "fold_valid",
)


@dataclass
class CircuitMetrics:
    circuit: str
    acir_opcodes: int
    backend_opcodes: int | None

    def to_json(self) -> dict[str, Any]:
        return {
            "circuit": self.circuit,
            "acir_opcodes": self.acir_opcodes,
            "backend_opcodes": self.backend_opcodes,
        }


def run_json(command: list[str], cwd: Path | None = None) -> dict[str, Any]:
    proc = subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(
            f"{' '.join(command)} failed with exit {proc.returncode}:\n{proc.stderr or proc.stdout}"
        )
    return json.loads(proc.stdout)


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def collect_acir(repo: Path, circuit: str, refresh: bool) -> int:
    circuit_dir = repo / "circuits" / circuit
    info_path = circuit_dir / "nargo_info.json"
    if refresh or not info_path.exists():
        if shutil.which("nargo") is None:
            raise RuntimeError(f"{info_path} missing and nargo is not on PATH")
        info = run_json(["nargo", "info", "--json"], cwd=circuit_dir)
        info_path.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
    else:
        info = read_json(info_path)

    functions = info.get("programs", [{}])[0].get("functions", [])
    for fn in functions:
        if fn.get("name") == "main" and fn.get("opcodes") is not None:
            return int(fn["opcodes"])
    raise RuntimeError(f"could not find main opcode count in {info_path}")


def collect_backend(repo: Path, circuit: str, refresh: bool) -> int | None:
    circuit_dir = repo / "circuits" / circuit
    gates_path = circuit_dir / "bb_gates.json"
    artifact = circuit_dir / "target" / f"{circuit}.json"

    if refresh or not gates_path.exists():
        if shutil.which("bb") is None:
            return None
        if not artifact.exists():
            raise RuntimeError(f"missing compiled artifact: {artifact}")
        gates = run_json(
            ["bb", "gates", "--scheme", "ultra_honk", "--bytecode_path", str(artifact)]
        )
        gates_path.write_text(json.dumps(gates, indent=2) + "\n", encoding="utf-8")
    else:
        gates = read_json(gates_path)

    functions = gates.get("functions", [])
    if not functions:
        raise RuntimeError(f"could not find backend gate data in {gates_path}")
    value = functions[0].get("circuit_size")
    return int(value) if value is not None else None


def collect_metrics(repo: Path, circuits: list[str], refresh: bool) -> list[CircuitMetrics]:
    metrics = []
    for circuit in circuits:
        metrics.append(
            CircuitMetrics(
                circuit=circuit,
                acir_opcodes=collect_acir(repo, circuit, refresh),
                backend_opcodes=collect_backend(repo, circuit, refresh),
            )
        )
    return metrics


def load_budgets(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise RuntimeError(f"budget file does not exist: {path}")
    return read_json(path)


def metrics_by_circuit(metrics: list[dict[str, Any]] | list[CircuitMetrics]) -> dict[str, dict[str, Any]]:
    by_name = {}
    for item in metrics:
        record = item.to_json() if isinstance(item, CircuitMetrics) else item
        by_name[record["circuit"]] = record
    return by_name


def compare(
    current: list[CircuitMetrics],
    budgets: dict[str, Any],
    baseline: dict[str, dict[str, Any]] | None,
) -> list[str]:
    failures: list[str] = []
    budget_map = budgets.get("circuits", {})
    default_delta = float(budgets.get("default_max_increase_pct", 0))

    for metric in current:
        budget = budget_map.get(metric.circuit, {})
        max_acir = budget.get("max_acir_opcodes")
        max_backend = budget.get("max_backend_opcodes")
        if max_acir is not None and metric.acir_opcodes > int(max_acir):
            failures.append(
                f"{metric.circuit}: ACIR opcodes {metric.acir_opcodes} exceed budget {max_acir}"
            )
        if (
            max_backend is not None
            and metric.backend_opcodes is not None
            and metric.backend_opcodes > int(max_backend)
        ):
            failures.append(
                f"{metric.circuit}: backend opcodes {metric.backend_opcodes} exceed budget {max_backend}"
            )

        if baseline and metric.circuit in baseline:
            allowed = float(budget.get("max_increase_pct", default_delta))
            base = baseline[metric.circuit]
            failures.extend(
                delta_failures(
                    metric.circuit,
                    "ACIR opcodes",
                    int(base["acir_opcodes"]),
                    metric.acir_opcodes,
                    allowed,
                )
            )
            if metric.backend_opcodes is not None and base.get("backend_opcodes") is not None:
                failures.extend(
                    delta_failures(
                        metric.circuit,
                        "backend opcodes",
                        int(base["backend_opcodes"]),
                        metric.backend_opcodes,
                        allowed,
                    )
                )

    return failures


def delta_failures(circuit: str, label: str, base: int, current: int, allowed_pct: float) -> list[str]:
    if base <= 0:
        return []
    pct = ((current - base) / base) * 100.0
    if pct > allowed_pct:
        return [
            f"{circuit}: {label} increased {pct:.2f}% ({base} -> {current}), "
            f"above {allowed_pct:.2f}% limit"
        ]
    return []


def write_report(
    out_dir: Path,
    metrics: list[CircuitMetrics],
    budgets: dict[str, Any],
    baseline: dict[str, dict[str, Any]] | None,
    failures: list[str],
) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "generated_at": int(time.time()),
        "git_sha": os.getenv("GITHUB_SHA", ""),
        "metrics": [m.to_json() for m in metrics],
        "failures": failures,
    }
    (out_dir / "circuit-constraints.json").write_text(
        json.dumps(payload, indent=2) + "\n", encoding="utf-8"
    )

    history_path = out_dir / "circuit-constraint-history.jsonl"
    with history_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, separators=(",", ":")) + "\n")

    (out_dir / "circuit-constraints.md").write_text(
        markdown_report(metrics, budgets, baseline, failures), encoding="utf-8"
    )
    (out_dir / "constraint-budget-chart.svg").write_text(
        svg_chart(metrics, budgets), encoding="utf-8"
    )


def markdown_report(
    metrics: list[CircuitMetrics],
    budgets: dict[str, Any],
    baseline: dict[str, dict[str, Any]] | None,
    failures: list[str],
) -> str:
    budget_map = budgets.get("circuits", {})
    lines = [
        "## Circuit Constraint Budget",
        "",
        "| Circuit | ACIR | ACIR Budget | Backend | Backend Budget | Baseline Delta |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for metric in metrics:
        budget = budget_map.get(metric.circuit, {})
        base = baseline.get(metric.circuit) if baseline else None
        delta = "-"
        if base:
            delta = format_delta(base.get("backend_opcodes"), metric.backend_opcodes)
        lines.append(
            "| `{}` | {} | {} | {} | {} | {} |".format(
                metric.circuit,
                metric.acir_opcodes,
                budget.get("max_acir_opcodes", "-"),
                metric.backend_opcodes if metric.backend_opcodes is not None else "unavailable",
                budget.get("max_backend_opcodes", "-"),
                delta,
            )
        )
    if failures:
        lines.extend(["", "### Failures", ""])
        lines.extend(f"- {failure}" for failure in failures)
    return "\n".join(lines) + "\n"


def format_delta(base: Any, current: Any) -> str:
    if base in (None, 0) or current is None:
        return "-"
    pct = ((int(current) - int(base)) / int(base)) * 100.0
    return f"{pct:+.2f}%"


def svg_chart(metrics: list[CircuitMetrics], budgets: dict[str, Any]) -> str:
    width = 920
    row_height = 58
    left = 170
    bar_width = 560
    height = 80 + len(metrics) * row_height
    budget_map = budgets.get("circuits", {})
    max_value = max(
        [
            int(budget_map.get(m.circuit, {}).get("max_backend_opcodes", 0) or 0)
            for m in metrics
        ]
        + [m.backend_opcodes or m.acir_opcodes for m in metrics]
        + [1]
    )
    rows = []
    for index, metric in enumerate(metrics):
        y = 50 + index * row_height
        backend = metric.backend_opcodes or 0
        budget = int(budget_map.get(metric.circuit, {}).get("max_backend_opcodes", 0) or 0)
        used_w = int((backend / max_value) * bar_width)
        budget_x = left + int((budget / max_value) * bar_width)
        color = "#d1495b" if budget and backend > budget else "#2f80ed"
        rows.append(
            f'<text x="20" y="{y + 19}" font-size="14" font-family="monospace">{metric.circuit}</text>'
        )
        rows.append(f'<rect x="{left}" y="{y}" width="{bar_width}" height="24" fill="#eef2f7"/>')
        rows.append(f'<rect x="{left}" y="{y}" width="{used_w}" height="24" fill="{color}"/>')
        rows.append(f'<line x1="{budget_x}" y1="{y - 6}" x2="{budget_x}" y2="{y + 30}" stroke="#111827" stroke-width="2"/>')
        rows.append(
            f'<text x="{left + bar_width + 20}" y="{y + 18}" font-size="13" font-family="sans-serif">{backend} / {budget}</text>'
        )
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">'
        '<rect width="100%" height="100%" fill="white"/>'
        '<text x="20" y="28" font-size="20" font-family="sans-serif" font-weight="700">Circuit backend constraint budget</text>'
        + "".join(rows)
        + "</svg>\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--budgets", type=Path, default=Path("circuits/constraint-budgets.json"))
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--output-dir", type=Path, default=Path("reports/circuits"))
    parser.add_argument("--refresh", action="store_true", help="rerun nargo info and bb gates")
    parser.add_argument("--circuits", nargs="*", default=list(DEFAULT_CIRCUITS))
    args = parser.parse_args()

    repo = args.repo.resolve()
    budgets_path = args.budgets if args.budgets.is_absolute() else repo / args.budgets
    out_dir = args.output_dir if args.output_dir.is_absolute() else repo / args.output_dir
    budgets = load_budgets(budgets_path)

    baseline = None
    if args.baseline:
        baseline_path = args.baseline if args.baseline.is_absolute() else repo / args.baseline
        baseline_payload = read_json(baseline_path)
        baseline = metrics_by_circuit(baseline_payload.get("metrics", []))

    metrics = collect_metrics(repo, args.circuits, args.refresh)
    failures = compare(metrics, budgets, baseline)
    write_report(out_dir, metrics, budgets, baseline, failures)

    summary = (out_dir / "circuit-constraints.md").read_text(encoding="utf-8")
    print(summary)
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
