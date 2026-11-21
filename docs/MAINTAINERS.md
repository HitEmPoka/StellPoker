# Maintainer Guide

How changes land in this repository: required status checks, the merge queue,
label triage, and review SLAs. Contributors should read
[CONTRIBUTING.md](../CONTRIBUTING.md) instead.

## Required status checks

Every pull request into `main` triggers the workflows listed under
`on: pull_request` in [`.github/workflows/`](../.github/workflows/). The
checks that gate merges:

| Check (workflow name) | What it verifies | Failing means |
|---|---|---|
| **CI** | Secret scan (gitleaks), `cargo fmt`, `cargo clippy`, contract/circuit/frontend tests, builds | Do not merge — fix or get an explicit override from a maintainer |
| **CodeQL Analysis — Rust Contracts** | Static analysis for security-detection rules | Investigate before merge; dismiss only with a written reason on the PR |
| **Dependency Check** | Advisory/audit pass over Rust and npm dependencies | Update or pin the dependency; `wontfix` label + comment if intentionally accepted |
| **Docker Image Vulnerability Scan** | Image CVE scan | Fix base image/deps; record accepted CVEs in the PR body |
| **Unsafe Code Audit** | Guards around `unsafe` usage in contracts/circuits | Revert `unsafe` additions unless the PR documents why it is unavoidable |
| **Soroban Budget Ceilings** | Resource-budget regression vs stored ceilings | Optimize or consciously raise the ceiling (see `scripts/check_soroban_budgets.py`) |
| **Storage Layout Snapshot** | Contract storage layout drift | Intentional layout changes need a matching snapshot update in the same PR |
| **Coordinator API Conformance** | Coordinator HTTP API against `docs/openapi.yaml` | Fix the implementation or update the spec in the same PR |
| **Showdown Winner Consistency** | Settlement correctness cross-check | Never merge on failure |
| **Benchmark Regression Detection** | Perf benchmarks vs baseline | Investigate large regressions; `benchmark` label + explanation in PR |
| **Coordinator Endpoint Fuzzing (PR)** | Targeted fuzzing of coordinator endpoints | Treat crashes as bugs; fix before merge |
| **Poker-Table Arg Validation Fuzz** / **Hand Evaluator Fuzzing** | Fuzzed contract/circuit surfaces | Same as above |
| **Circuit Benchmarks** / **Circuit Gas Benchmarks** / **Cost Model** | Circuit perf and cost-model drift | Review circuit changes with extra scrutiny |

Recommended branch-protection configuration for `main`
(Settings → Branches → Branch protection rules):

- Require a pull request before merging (1 approving review).
- **Require status checks to pass before merging** — add at minimum:
  `CI`, `CodeQL Analysis — Rust Contracts`, `Dependency Check`,
  `Unsafe Code Audit`, `Soroban Budget Ceilings`, `Showdown Winner Consistency`.
- Require branches to be up to date before merging.
- Do not allow force pushes or branch deletion on `main`.

## Merge queue

With the merge queue enabled (Settings → General → Pull Requests →
**Merge queue**), a green PR is added to a batch rather than merged directly:

1. Mark the PR **Merge when ready** (or add the `queue` label, if configured).
2. The queue builds a speculative merge of the batch against `main`; all
   required checks must pass on the merge commit.
3. If anything fails, only the responsible PRs are kicked out of the queue —
   rebase, fix, and re-queue.
4. Squash-merge is the default; keep the squash subject as
  `Fix #N — short summary` and ensure the description contains the
  `Closes #N` lines so issues close on merge.

Rules of thumb:

- Never bypass the queue to merge a red PR. Emergency hotfixes use the same
  queue with a maintainer explicitly paged (see
  [docs/disaster-recovery.md](disaster-recovery.md)).
- If your PR only touches docs, the queue still runs the full check set —
  do not force-merge to "save time".

## Label triage

Apply labels when the PR/issue is first read, not at merge time.

| Label | Apply when |
|---|---|
| `bug` | Something incorrect or crashing; include repro steps |
| `enhancement` | New capability or behavior change |
| `documentation` | Docs-only change (this label also marks docs-only PRs as safe to queue quickly) |
| `contracts` / `circuits` / `frontend` | Code area — exactly one required, more allowed |
| `mpc` | Committee, node, key-management, or session-protocol surface |
| `security` | Crypto, auth, secret handling, or anything flagged by CodeQL/Dependency Check |
| `testing` | Test-only changes, fuzzers, fixtures |
| `developer-experience` | Tooling, scripts, Makefile, CI ergonomics |
| `good first issue` / `help wanted` | Scoped so an outside contributor can finish it |
| `question` / `invalid` / `duplicate` / `wontfix` | Issue hygiene (close-via-label is fine, leave a comment) |

## Review SLAs

Clock is business days (Mon–Fri), measured from first request to first
substantive response.

| Event | Target |
|---|---|
| First response on a new PR (triage + labels + request changes/approve) | 2 business days |
| Security-labeled PRs (`security`, CodeQL/Dependency Check failures) | 24 hours |
| Re-review after the author addresses feedback | 1 business day |
| Docs-only PRs | 2 business days (still needs 1 approval) |
| Issues: triage + label or close | 3 business days |

If a review misses its SLA, the author may request re-review once; a
maintainer who cannot review in time hands off to another maintainer rather
than letting the PR stall.

## Merging checklist

1. All required checks green on the latest commit.
2. At least one approving review (not the author).
3. `Closes #N` lines present in the PR description.
4. Labels from the triage table applied.
5. Squash-merge via the queue; delete the branch after merge.
