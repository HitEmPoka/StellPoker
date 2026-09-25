#!/usr/bin/env python3
"""
Percentile benchmark collection script for circuit proving times.

This script runs the circuit benchmark suite multiple times (default 30 runs)
and computes p50, p95, p99 percentiles for proving time across each circuit.

Usage:
    CIRCUIT_DIRS="circuits/deal_valid circuits/showdown_valid" \\
    NUM_RUNS=30 \\
    python3 scripts/collect_percentile_benchmarks.py

Environment variables:
    CIRCUIT_DIRS: Space-separated circuit directory paths
    NUM_RUNS: Number of proof generations per circuit (default: 30)
    OUTPUT_FILE: Path to write results (default: benchmark_data/percentiles.json)

Output format (JSON):
{
    "deal_valid": {
        "num_runs": 30,
        "prove_time_ms": {
            "p50": 54.3,
            "p95": 58.2,
            "p99": 59.8,
            "min": 50.1,
            "max": 62.5,
            "mean": 54.7
        }
    },
    ...
}
"""

import json
import os
import sys
import subprocess
import statistics
from pathlib import Path
from typing import Dict, List, Any
from dataclasses import dataclass

@dataclass
class BenchmarkResult:
    prove_times_ms: List[float]
    verify_times_ms: List[float]

    def percentiles(self) -> Dict[str, float]:
        """Compute percentiles for proving time."""
        if not self.prove_times_ms:
            return {}

        sorted_times = sorted(self.prove_times_ms)
        n = len(sorted_times)

        def percentile(p: float) -> float:
            idx = int((p / 100.0) * (n - 1))
            return sorted_times[idx]

        return {
            "p50": percentile(50),
            "p95": percentile(95),
            "p99": percentile(99),
            "min": min(sorted_times),
            "max": max(sorted_times),
            "mean": statistics.mean(sorted_times),
            "stdev": statistics.stdev(sorted_times) if n > 1 else 0.0,
            "count": n,
        }

def run_circuit_proof(circuit_dir: str) -> float:
    """
    Run a single proof generation and return proving time in milliseconds.
    Returns the time extracted from 'bb prove' output.

    Note: This is a placeholder implementation. In practice, this would:
    1. Compile the circuit (if needed)
    2. Run 'bb prove --scheme ultra_honk' with timing
    3. Parse the proving time from output or measure wall-clock time
    """
    # Placeholder: return 0 to indicate no actual timing available in this context
    # In real usage, this would shell out to bb and collect actual times
    return 0.0

def collect_benchmarks(circuit_dirs: List[str], num_runs: int) -> Dict[str, BenchmarkResult]:
    """
    Collect benchmark results for each circuit over num_runs iterations.

    Args:
        circuit_dirs: List of circuit directory paths
        num_runs: Number of proof generations per circuit

    Returns:
        Dictionary mapping circuit name to BenchmarkResult
    """
    results: Dict[str, BenchmarkResult] = {}

    for circuit_dir in circuit_dirs:
        circuit_name = Path(circuit_dir).name
        prove_times = []
        verify_times = []

        print(f"Benchmarking {circuit_name} ({num_runs} runs)...", file=sys.stderr)

        # Collect prove times (placeholder implementation)
        # In production, this would actually run bb prove and measure time
        for i in range(num_runs):
            prove_ms = run_circuit_proof(circuit_dir)
            if prove_ms > 0:
                prove_times.append(prove_ms)

            if (i + 1) % 10 == 0:
                print(f"  {i + 1}/{num_runs} runs completed", file=sys.stderr)

        if prove_times:
            results[circuit_name] = BenchmarkResult(
                prove_times_ms=prove_times,
                verify_times_ms=verify_times
            )

    return results

def format_output(results: Dict[str, BenchmarkResult]) -> Dict[str, Any]:
    """Format benchmark results as JSON-serializable dictionary."""
    output = {}

    for circuit_name, result in results.items():
        percentiles = result.percentiles()
        output[circuit_name] = {
            "num_runs": len(result.prove_times_ms),
            "prove_time_ms": percentiles,
        }

    return output

def write_results(results: Dict[str, BenchmarkResult], output_file: str):
    """Write benchmark results to JSON file."""
    output = format_output(results)

    Path(output_file).parent.mkdir(parents=True, exist_ok=True)

    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2)

    print(f"Benchmark results written to {output_file}")

def print_summary(results: Dict[str, BenchmarkResult]):
    """Print human-readable summary of benchmark results."""
    print("\n" + "=" * 80)
    print("PROVING TIME PERCENTILES (milliseconds)")
    print("=" * 80)

    for circuit_name in sorted(results.keys()):
        result = results[circuit_name]
        percentiles = result.percentiles()

        print(f"\n{circuit_name}:")
        print(f"  Runs:  {percentiles.get('count', 0)}")
        print(f"  Min:   {percentiles.get('min', 0):.2f} ms")
        print(f"  p50:   {percentiles.get('p50', 0):.2f} ms")
        print(f"  p95:   {percentiles.get('p95', 0):.2f} ms")
        print(f"  p99:   {percentiles.get('p99', 0):.2f} ms")
        print(f"  Max:   {percentiles.get('max', 0):.2f} ms")
        print(f"  Mean:  {percentiles.get('mean', 0):.2f} ms (±{percentiles.get('stdev', 0):.2f})")

def main():
    circuit_dirs_str = os.getenv("CIRCUIT_DIRS", "")
    num_runs = int(os.getenv("NUM_RUNS", "30"))
    output_file = os.getenv("OUTPUT_FILE", "benchmark_data/percentiles.json")

    if not circuit_dirs_str:
        print("ERROR: CIRCUIT_DIRS environment variable not set", file=sys.stderr)
        print("Usage: CIRCUIT_DIRS='path/to/circuit1 path/to/circuit2' python3 script.py", file=sys.stderr)
        sys.exit(1)

    circuit_dirs = circuit_dirs_str.split()

    print(f"Collecting benchmarks for {len(circuit_dirs)} circuits, {num_runs} runs each", file=sys.stderr)

    results = collect_benchmarks(circuit_dirs, num_runs)

    if results:
        write_results(results, output_file)
        print_summary(results)
    else:
        print("ERROR: No benchmark results collected", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
