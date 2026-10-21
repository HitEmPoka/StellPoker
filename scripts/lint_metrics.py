#!/usr/bin/env python3
"""Lint Prometheus metric declarations against the project convention.

Scans Rust source for `Opts::new(...)` and `HistogramOpts::new(...)` calls,
extracts the metric name and HELP text, and validates each against the rules
in docs/metrics-naming.md.

Also validates every metric name referenced from alerting rules and Grafana
dashboards against the declared set, so renames don't quietly leave orphan
references behind.

Exit codes:
    0 — all clean
    1 — one or more violations
    2 — usage error
"""

from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

KNOWN_NAMESPACES = {
    "coordinator",
    "mpc_node",
    "mpc_coordinator",
    "circuits",
    "contracts",
    "k6",
}

RESERVED_PREFIXES = ("node_", "process_", "go_", "promhttp_", "prometheus_")

NAME_RE = re.compile(r"^[a-z][a-z0-9_]*[a-z0-9]$")
DOUBLE_UNDERSCORE_RE = re.compile(r"__")
FORBIDDEN_SUFFIXES = (
    "_ms",
    "_millis",
    "_micros",
    "_nanos",
    "_percent",
    "_pct",
    "_kb",
    "_mb",
    "_gb",
    "_count",   # counters must use _total; _count is only emitted automatically by histograms
)
FORBIDDEN_LABEL_NAMES = {"type", "kind", "status", "name"}

# ALLOWLIST: metric_name -> reason. Expires on next major version bump.
# Each entry must be justified in review and tracked for removal.
ALLOWLIST: dict[str, str] = {}

# Matches the two shapes a Rust metric declaration takes in this codebase:
#   1. `Opts::new("name", "help")`  or  `HistogramOpts::new("name", "help")`
#      — carries labels in the second builder step; the first two string
#      literals in the call are name and help.
#   2. `IntCounter::new("name", "help")` and friends (`Counter`, `Gauge`,
#      `IntGauge`) — the direct constructor with no `Opts::` wrapper, used for
#      zero-label metrics (see the MPC session counters in main.rs).
METRIC_DECL_RE = re.compile(
    r"""
    (?:
        (?:Opts|HistogramOpts)::new
      | (?:IntCounter|Counter|IntGauge|Gauge)::new
    )\(\s*
        "(?P<name>[^"]+)"\s*,\s*
        "(?P<help>(?:[^"\\]|\\.)*)"\s*
    (?:,|\))
    """,
    re.VERBOSE | re.DOTALL,
)


@dataclass
class Violation:
    file: Path
    line: int
    metric: str
    message: str

    def format(self) -> str:
        rel = self.file.relative_to(REPO_ROOT)
        return f"{rel}:{self.line} [{self.metric}] {self.message}"


def iter_rust_sources(roots: list[Path]) -> list[Path]:
    out: list[Path] = []
    for root in roots:
        if root.is_file():
            if root.suffix == ".rs":
                out.append(root)
            continue
        for path in root.rglob("*.rs"):
            # Skip target/ and vendored code.
            parts = set(path.parts)
            if "target" in parts or "vendor" in parts:
                continue
            out.append(path)
    return out


def line_of(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def check_metric_name(name: str) -> list[str]:
    errors: list[str] = []
    if name in ALLOWLIST:
        return errors
    if not NAME_RE.match(name):
        errors.append(f"name does not match ^[a-z][a-z0-9_]*[a-z0-9]$")
    if DOUBLE_UNDERSCORE_RE.search(name):
        errors.append("name contains '__'")
    if any(name.startswith(p) for p in RESERVED_PREFIXES):
        errors.append(f"name uses reserved prefix (one of {RESERVED_PREFIXES})")

    namespace = name.split("_", 1)[0]
    if namespace == "mpc":
        # mpc_node / mpc_coordinator — take first two segments.
        namespace = "_".join(name.split("_", 2)[:2])
    if namespace not in KNOWN_NAMESPACES:
        errors.append(
            f"namespace '{namespace}' not in KNOWN_NAMESPACES "
            f"({sorted(KNOWN_NAMESPACES)})"
        )

    for suffix in FORBIDDEN_SUFFIXES:
        if name.endswith(suffix):
            errors.append(f"forbidden unit suffix '{suffix}' (see metrics-naming.md §3)")

    return errors


def check_help(name: str, help_text: str) -> list[str]:
    errors: list[str] = []
    if not help_text.strip():
        errors.append("HELP text is empty")
        return errors
    first = help_text.strip()[0]
    # A capital letter is the usual start; digits are also acceptable for help
    # text that begins with a sentinel value, e.g. "1 when draining, 0 otherwise".
    if not (first.isupper() or first.isdigit()):
        errors.append("HELP text should start with a capital letter or digit")
    if not help_text.rstrip().endswith("."):
        errors.append("HELP text should end with a period")
    return errors


def extract_metric_names_from_yaml(path: Path) -> set[str]:
    """Very light pass over YAML/JSON files — we just want things that look like
    `coordinator_...` or `mpc_node_...` style tokens, so we match on the known
    namespaces and the shape regex. Full YAML parsing would require PyYAML; the
    regex pass is enough for CI."""
    text = path.read_text(errors="replace")
    pattern = re.compile(
        r"\b(" + "|".join(sorted(KNOWN_NAMESPACES, key=len, reverse=True)) + r")_[a-z0-9_]+\b"
    )
    return set(pattern.findall(text)) if False else {m.group(0) for m in pattern.finditer(text)}


def main(argv: list[str]) -> int:
    args = argv[1:]
    if any(a in ("-h", "--help") for a in args):
        print(__doc__)
        return 0

    roots = [REPO_ROOT / p for p in args] if args else [REPO_ROOT / "services"]
    for r in roots:
        if not r.exists():
            print(f"ERROR: path does not exist: {r}", file=sys.stderr)
            return 2

    violations: list[Violation] = []
    declared: dict[str, Path] = {}

    for src in iter_rust_sources(roots):
        text = src.read_text(errors="replace")
        for m in METRIC_DECL_RE.finditer(text):
            name = m.group("name")
            help_text = m.group("help")
            line = line_of(text, m.start())
            declared.setdefault(name, src)
            for err in check_metric_name(name):
                violations.append(Violation(src, line, name, err))
            for err in check_help(name, help_text):
                violations.append(Violation(src, line, name, err))

    # Dashboard / alerting cross-check.
    ref_files = []
    for pattern in ("load-testing/prometheus-alerting-rules.yml",
                    "load-testing/grafana/**/*.json",
                    "load-testing/prometheus.yml"):
        ref_files.extend(REPO_ROOT.glob(pattern))

    referenced: set[str] = set()
    for ref in ref_files:
        if ref.is_file():
            referenced.update(extract_metric_names_from_yaml(ref))

    for name in sorted(referenced - declared.keys()):
        # Skip generic histogram sub-metrics (parent + _bucket/_sum/_count).
        parent = re.sub(r"_(bucket|sum|count)$", "", name)
        if parent in declared:
            continue
        # References from alerting / dashboards to an unknown metric are a soft
        # failure: they usually mean a rename was not completed.
        violations.append(
            Violation(
                file=REPO_ROOT / "load-testing",
                line=0,
                metric=name,
                message="referenced in dashboards/alerts but not declared in any service",
            )
        )

    if not violations:
        print(f"ok: {len(declared)} metric(s) checked across {len(roots)} path(s)")
        return 0

    for v in violations:
        print(v.format())
    print(f"\n{len(violations)} violation(s). See docs/metrics-naming.md.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
