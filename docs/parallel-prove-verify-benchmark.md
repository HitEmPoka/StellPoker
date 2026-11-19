# Parallel Prove+Verify Benchmark (Issue #644)

This benchmark measures local proof generation and verification latency under
20 concurrent hands. It publishes p50/p95/p99 for proving, verification, and
combined prove+verify latency, with hardware and toolchain metadata attached to
every result.

## Toolchain

Real measurements require `bb` on `PATH` and compiled circuit artifacts at
`circuits/<name>/target/<name>.json` and `<name>.gz`. The CI workflow runs on a
self-hosted runner labelled `benchmarks` so hardware is stable across runs.

A synthetic smoke run is available for script validation only:

```bash
python3 scripts/bench_parallel_prove_verify.py --self-test
```

Self-test output is labelled `measurement_mode: self-test` and must not be used
as a performance baseline or CI gate.

## Run locally

```bash
CIRCUITS="circuits/deal_valid circuits/showdown_valid" \
CONCURRENCY=20 \
RUNS_PER_CIRCUIT=10 \
python3 scripts/bench_parallel_prove_verify.py
```

Outputs:

- `benchmark_data/parallel_prove_verify.json`
- `benchmark_data/parallel_prove_verify_report.md`

The report includes OS, CPU, logical CPU count, memory, `bb --version`, sample
count, p50/p95/p99 for each metric, and a recommended `SESSION_TIMEOUT_SECS`.

## Timeout configuration

The recommendation is:

```text
max(60, ceil(worst-circuit p99 combined latency × TIMEOUT_SAFETY_FACTOR / 1000))
```

`TIMEOUT_SAFETY_FACTOR` defaults to `3`. Copy the printed
`SESSION_TIMEOUT_SECS=<value>` into `.env.staging` and the coordinator
environment. The coordinator reads this value in
`services/coordinator/src/session_gc.rs`; the built-in default is 300 seconds.

## CI gate

Dispatch `Parallel Prove Verify Benchmark` with the desired concurrency, proof
count, and optional `max_p95_total_ms`. When `max_p95_total_ms` is non-zero, the
job fails if the worst circuit's p95 combined latency exceeds the budget. The
workflow publishes the report to the job summary and uploads both artifacts for
90 days.

No simulated values are accepted as real benchmarks; if `bb` is missing the
real benchmark exits with an error instead of inventing data.
