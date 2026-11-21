#!/usr/bin/env python3
import argparse
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple


def percentile(values: List[float], p: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = math.ceil((p / 100.0) * len(ordered)) - 1
    return ordered[max(0, min(index, len(ordered) - 1))]


def summarize(values: List[float]) -> Dict[str, float]:
    return {
        "p50": round(percentile(values, 50), 2),
        "p95": round(percentile(values, 95), 2),
        "p99": round(percentile(values, 99), 2),
        "min": round(min(values), 2) if values else 0.0,
        "max": round(max(values), 2) if values else 0.0,
        "mean": round(statistics.mean(values), 2) if values else 0.0,
        "count": len(values),
    }


def run(cmd: List[str], cwd: Optional[Path] = None) -> None:
    result = subprocess.run(
        [str(part) for part in cmd],
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        timeout=300,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed: {' '.join(str(part) for part in cmd)}\n{result.stderr}"
        )


def command_version(binary: str) -> str:
    try:
        result = subprocess.run(
            [binary, "--version"], capture_output=True, text=True, timeout=15
        )
        lines = (result.stdout or result.stderr).strip().splitlines()
        return lines[0] if lines else "unknown"
    except Exception:
        return "unknown"


def hardware_specs() -> Dict[str, object]:
    cpu_model = platform.processor() or "unknown"
    if sys.platform == "darwin":
        try:
            cpu_model = subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
            ).strip()
        except Exception:
            pass

    memory_bytes = None
    if sys.platform == "darwin":
        try:
            memory_bytes = int(subprocess.check_output(["sysctl", "-n", "hw.memsize"]).strip())
        except Exception:
            pass
    else:
        try:
            for line in Path("/proc/meminfo").read_text().splitlines():
                if line.startswith("MemTotal:"):
                    memory_bytes = int(line.split()[1]) * 1024
                    break
        except Exception:
            pass

    return {
        "os": f"{platform.system()} {platform.release()}",
        "arch": platform.machine(),
        "cpu_model": cpu_model,
        "logical_cpus": os.cpu_count() or 0,
        "memory_bytes": memory_bytes,
        "python": platform.python_version(),
    }


def write_vk(circuit: str, artifact: Path, work_root: Path) -> Path:
    vk_dir = work_root / f"{circuit}-vk"
    vk_dir.mkdir(parents=True, exist_ok=True)
    run(["bb", "write_vk", "--scheme", "ultra_honk", "-b", artifact, "-o", vk_dir])
    vk_path = vk_dir / "vk"
    if not vk_path.exists():
        raise RuntimeError(f"verification key not created for {circuit}")
    return vk_path


def measure_once(
    circuit: str,
    index: int,
    artifact: Path,
    witness: Path,
    vk_path: Path,
    work_root: Path,
    mode: str,
) -> Dict[str, float]:
    if mode == "self-test":
        prove_ms = 40.0 + ((index * 7) % 23)
        verify_ms = 6.0 + ((index * 3) % 11)
    else:
        out_dir = work_root / f"{circuit}-{index}"
        out_dir.mkdir(parents=True, exist_ok=True)
        started = time.perf_counter()
        run(
            [
                "bb", "prove", "--scheme", "ultra_honk",
                "-b", artifact, "-w", witness, "-k", vk_path, "-o", out_dir,
            ]
        )
        prove_ms = (time.perf_counter() - started) * 1000
        started = time.perf_counter()
        run(
            [
                "bb", "verify", "--scheme", "ultra_honk",
                "-k", vk_path, "-p", out_dir / "proof", "-i", out_dir / "public_inputs",
            ]
        )
        verify_ms = (time.perf_counter() - started) * 1000

    return {
        "circuit": circuit,
        "prove_ms": prove_ms,
        "verify_ms": verify_ms,
        "total_ms": prove_ms + verify_ms,
    }


def collect(circuits: List[str], concurrency: int, runs: int, mode: str, work_root: Path) -> Dict[str, List[Dict[str, float]]]:
    samples: Dict[str, List[Dict[str, float]]] = {circuit: [] for circuit in circuits}
    jobs: List[Tuple[str, int, Path, Path, Path]] = []

    for circuit in circuits:
        directory = Path(circuit)
        artifact = directory / "target" / f"{directory.name}.json"
        witness = directory / "target" / f"{directory.name}.gz"
        if mode != "self-test":
            if not artifact.exists() or not witness.exists():
                raise RuntimeError(f"missing artifacts for {circuit}: {artifact}, {witness}")
            vk_path = write_vk(circuit, artifact, work_root)
        else:
            artifact = directory
            witness = directory
            vk_path = directory
        for index in range(runs):
            jobs.append((circuit, index, artifact, witness, vk_path))

    with ThreadPoolExecutor(max_workers=concurrency) as executor:
        futures = [
            executor.submit(measure_once, circuit, index, artifact, witness, vk_path, work_root, mode)
            for circuit, index, artifact, witness, vk_path in jobs
        ]
        for future in as_completed(futures):
            sample = future.result()
            samples[sample["circuit"]].append(sample)

    return samples


def format_report(
    results: Dict[str, Dict[str, Dict[str, float]]],
    specs: Dict[str, object],
    metadata: Dict[str, object],
    recommendation: Dict[str, object],
) -> str:
    lines = [
        "# Parallel prove+verify benchmark",
        "",
        f"Generated: {metadata['generated_at']}",
        "",
        f"Measurement mode: `{metadata['measurement_mode']}`",
        "",
        "## Hardware",
        "",
        f"- OS: {specs['os']} ({specs['arch']})",
        f"- CPU: {specs['cpu_model']}",
        f"- Logical CPUs: {specs['logical_cpus']}",
        f"- Memory: {specs['memory_bytes']} bytes",
        f"- bb: {metadata['bb_version']}",
        "",
        f"Concurrency: {metadata['concurrency']} hands",
        "",
        "## Percentiles (ms)",
        "",
        "| Circuit | Metric | p50 | p95 | p99 | samples |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for circuit in sorted(results):
        for metric in ("prove_ms", "verify_ms", "total_ms"):
            stats = results[circuit][metric]
            lines.append(
                f"| `{circuit}` | {metric} | {stats['p50']:.2f} | {stats['p95']:.2f} | "
                f"{stats['p99']:.2f} | {stats['count']} |"
            )

    lines.extend(
        [
            "",
            "## Coordinator timeout recommendation",
            "",
            f"- Recommended `SESSION_TIMEOUT_SECS`: `{recommendation['session_timeout_secs']}`",
            f"- Basis: worst-circuit p99 total × {recommendation['safety_factor']}",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    mode = "self-test" if args.self_test or os.getenv("BENCH_MODE") == "self-test" else "real"
    circuits = os.getenv("CIRCUITS", "circuits/deal_valid circuits/showdown_valid").split()
    concurrency = int(os.getenv("CONCURRENCY", "20"))
    runs = int(os.getenv("RUNS_PER_CIRCUIT", "10"))
    output = Path(os.getenv("OUTPUT_FILE", "benchmark_data/parallel_prove_verify.json"))
    report_path = Path(os.getenv("REPORT_FILE", "benchmark_data/parallel_prove_verify_report.md"))
    safety_factor = float(os.getenv("TIMEOUT_SAFETY_FACTOR", "3"))
    max_p95_ms = float(os.getenv("MAX_P95_TOTAL_MS", "0"))

    if mode == "real" and shutil.which("bb") is None:
        print("bb is required for real measurements; use --self-test for a synthetic smoke run", file=sys.stderr)
        return 2

    work_root = Path(os.getenv("WORK_ROOT", "/tmp/stellpoker-parallel-prove-verify"))
    work_root.mkdir(parents=True, exist_ok=True)

    try:
        samples = collect(circuits, concurrency, runs, mode, work_root)
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1

    results: Dict[str, Dict[str, Dict[str, float]]] = {}
    for circuit, circuit_samples in samples.items():
        results[circuit] = {
            "prove_ms": summarize([sample["prove_ms"] for sample in circuit_samples]),
            "verify_ms": summarize([sample["verify_ms"] for sample in circuit_samples]),
            "total_ms": summarize([sample["total_ms"] for sample in circuit_samples]),
        }

    worst_p99_total_ms = max(
        (result["total_ms"]["p99"] for result in results.values()), default=0.0
    )
    worst_p95_total_ms = max(
        (result["total_ms"]["p95"] for result in results.values()), default=0.0
    )
    recommended_timeout = max(60, math.ceil(worst_p99_total_ms * safety_factor / 1000))
    recommendation = {
        "session_timeout_secs": recommended_timeout,
        "safety_factor": safety_factor,
        "worst_p99_total_ms": round(worst_p99_total_ms, 2),
        "worst_p95_total_ms": round(worst_p95_total_ms, 2),
    }

    metadata = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "measurement_mode": mode,
        "concurrency": concurrency,
        "runs_per_circuit": runs,
        "bb_version": command_version("bb") if mode == "real" else "not-used",
    }
    payload = {
        "metadata": metadata,
        "hardware": hardware_specs(),
        "results": results,
        "recommendation": recommendation,
    }

    output.parent.mkdir(parents=True, exist_ok=True)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    report_path.write_text(format_report(results, payload["hardware"], metadata, recommendation) + "\n", encoding="utf-8")

    print(f"Wrote {output} and {report_path}")
    print(f"SESSION_TIMEOUT_SECS={recommended_timeout}")
    if max_p95_ms > 0 and worst_p95_total_ms > max_p95_ms:
        print(
            f"p95 total {worst_p95_total_ms:.2f} ms exceeds MAX_P95_TOTAL_MS={max_p95_ms:.2f}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
