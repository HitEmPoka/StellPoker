#!/usr/bin/env python3
"""
Coordinator API OpenAPI conformance test.

Two modes:
  1. Offline (default / CI without a live coordinator):
     Validates the spec file itself for structural correctness using
     the schemathesis schema-loader.

  2. Live (requires a running coordinator):
     Set COORDINATOR_URL=http://localhost:8080 and the script will
     run schemathesis against the live server, checking:
       - not_a_server_error   (no 5xx from well-formed requests)
       - response_schema_conformance  (response bodies match the spec)
       - status_code_conformance      (only documented status codes returned)

Usage
-----
# Offline spec validation only (used in CI on every push):
  python3 scripts/check-api-spec.py

# Live conformance test against a running coordinator (used in e2e CI):
  COORDINATOR_URL=http://localhost:8080 python3 scripts/check-api-spec.py

Exit codes
----------
  0  all checks passed
  1  one or more checks failed
"""

import os
import sys
import pathlib
import subprocess
import re
import json

SPEC_PATH = pathlib.Path(__file__).parent.parent / "docs" / "openapi.yaml"
PUBLISHED_SPEC_PATH = pathlib.Path(__file__).parent.parent / "docs" / "api" / "openapi.yaml"
COORDINATOR_URL = os.environ.get("COORDINATOR_URL", "")

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def load_spec():
    """Parse and return the raw OpenAPI YAML document as a dict."""
    try:
        import yaml  # PyYAML
    except ImportError:
        print("ERROR: PyYAML not installed. Run: pip install pyyaml", file=sys.stderr)
        sys.exit(1)

    with open(SPEC_PATH) as f:
        return yaml.safe_load(f)


def validate_spec_structure(spec: dict) -> bool:
    """
    Minimal structural validation so we catch obvious spec regressions.
    Returns True if spec is valid, False otherwise.
    """
    ok = True
    errors = []

    # Top-level required keys
    for key in ("openapi", "info", "paths"):
        if key not in spec:
            errors.append(f"Missing top-level key: '{key}'")
            ok = False

    # Every path must have at least one HTTP method with a 'responses' block
    for path, path_item in spec.get("paths", {}).items():
        for method in ("get", "post", "put", "patch", "delete"):
            op = path_item.get(method)
            if op is None:
                continue
            if "responses" not in op:
                errors.append(f"  {method.upper()} {path}: missing 'responses'")
                ok = False
            if "operationId" not in op:
                errors.append(f"  {method.upper()} {path}: missing 'operationId' (recommended)")
                # not a hard failure

    # Check all $ref targets exist inside the same document
    import json
    spec_text = json.dumps(spec)
    components = spec.get("components", {}).get("schemas", {})
    params    = spec.get("components", {}).get("parameters", {})

    import re
    for ref in re.findall(r'"#/components/schemas/([^"]+)"', spec_text):
        if ref not in components:
            errors.append(f"  Broken $ref: #/components/schemas/{ref}")
            ok = False
    for ref in re.findall(r'"#/components/parameters/([^"]+)"', spec_text):
        if ref not in params:
            errors.append(f"  Broken $ref: #/components/parameters/{ref}")
            ok = False

    # Print findings
    if errors:
        for e in errors:
            print(f"[SPEC ERROR] {e}", file=sys.stderr)
    else:
        print("[OK] Spec structure is valid.")

    return ok


MAIN_RS_PATH = pathlib.Path(__file__).parent.parent / "services" / "coordinator" / "src" / "main.rs"


def parse_coordinator_routes(content: str) -> dict:
    """Extract Axum .route(...) mappings from coordinator main.rs."""
    routes = {}
    idx = 0
    while True:
        pos = content.find(".route(", idx)
        if pos == -1:
            break
        open_pos = pos + len(".route(")
        depth = 1
        i = open_pos
        in_string = False
        escape = False
        while i < len(content) and depth > 0:
            c = content[i]
            if escape:
                escape = False
            elif c == '\\':
                escape = True
            elif c == '"':
                in_string = not in_string
            elif not in_string:
                if c == '(':
                    depth += 1
                elif c == ')':
                    depth -= 1
            i += 1
        if depth == 0:
            inner = content[open_pos:i-1].strip()
            str_match = re.match(r'^"([^"]+)"\s*,\s*(.*)$', inner, re.DOTALL)
            if str_match:
                path = str_match.group(1).strip()
                handler_expr = str_match.group(2).strip()
                for method in ("get", "post", "put", "patch", "delete"):
                    if re.search(r'(?:axum::routing::|\b)' + method + r'\(', handler_expr) or re.search(r'\.' + method + r'\(', handler_expr):
                        routes[(method, path)] = handler_expr
        idx = pos + 7
    return routes


def validate_handler_parity(spec: dict) -> bool:
    """
    Assert zero drift between OpenAPI spec operations and coordinator Axum routes.
    Fails when implementation handlers diverge from OpenAPI.
    """
    if not MAIN_RS_PATH.exists():
        print(f"[WARN] Coordinator main.rs not found at {MAIN_RS_PATH}; skipping parity check.")
        return True

    with open(MAIN_RS_PATH, encoding="utf-8") as f:
        main_content = f.read()

    impl_routes = parse_coordinator_routes(main_content)
    spec_routes = {}
    for path, path_item in spec.get("paths", {}).items():
        # OpenAPI parameter format {param} matches Axum format :param
        axum_path = re.sub(r'\{([^}]+)\}', r':\1', path)
        for method, op in path_item.items():
            if method.lower() in ("get", "post", "put", "patch", "delete"):
                spec_routes[(method.lower(), axum_path)] = {
                    "original_path": path,
                    "operation_id": op.get("operationId", "unnamed"),
                }

    missing_in_impl = []
    for route, meta in spec_routes.items():
        if route not in impl_routes:
            missing_in_impl.append(f"{route[0].upper()} {meta['original_path']} (operationId: {meta['operation_id']})")

    if missing_in_impl:
        print("[FAIL] OpenAPI spec contains operations missing from coordinator implementation:", file=sys.stderr)
        for missing in missing_in_impl:
            print(f"  - {missing}", file=sys.stderr)
        return False

    print(f"[OK] Handler parity validated: all {len(spec_routes)} OpenAPI operations match coordinator router.")
    return True


HTTP_METHODS = ("get", "post", "put", "patch", "delete")


def spec_route_keys(spec: dict) -> set:
    """Return (method, axum-style-path) tuples declared in the spec."""
    keys = set()
    for path, path_item in spec.get("paths", {}).items():
        axum_path = re.sub(r"\{([^}]+)\}", r":\1", path)
        for method, op in path_item.items():
            if method.lower() in HTTP_METHODS:
                keys.add((method.lower(), axum_path))
    return keys


def route_exclusions(spec: dict) -> list:
    """Validate and return the documented route exclusions from the spec.

    Each rule must carry a `reason` so that an exclusion is always a written
    decision rather than a silent way to make the drift check pass.
    """
    rules = spec.get("x-route-exclusions", [])
    for rule in rules:
        if not isinstance(rule, dict) or not rule.get("reason"):
            print(
                "[SPEC ERROR] Every x-route-exclusions entry needs a 'reason'.",
                file=sys.stderr,
            )
            return []
        if not any(key in rule for key in ("exact", "prefix", "suffix")):
            print(
                "[SPEC ERROR] Every x-route-exclusions entry needs one of "
                "'exact', 'prefix' or 'suffix'.",
                file=sys.stderr,
            )
            return []
    return rules


def _as_list(value) -> list:
    """Accept either a scalar or a list in the exclusion YAML."""
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    return list(value)


def is_excluded(method: str, path: str, rules: list) -> bool:
    key = f"{method.upper()} {path}"
    for rule in rules:
        if key in _as_list(rule.get("exact")):
            return True
        if path.startswith(tuple(_as_list(rule.get("prefix")))):
            return True
        if path.endswith(tuple(_as_list(rule.get("suffix")))):
            return True
    return False


def validate_route_coverage(spec: dict) -> bool:
    """Fail when a registered coordinator route is absent from the spec.

    `validate_handler_parity` only catches spec -> router drift. This check
    catches the opposite (and much more common) direction: a new route being
    added to the Axum router without an OpenAPI operation or a documented
    exclusion. Issue #1011.
    """
    if not MAIN_RS_PATH.exists():
        print(f"[WARN] Coordinator main.rs not found at {MAIN_RS_PATH}; skipping coverage check.")
        return True

    with open(MAIN_RS_PATH, encoding="utf-8") as f:
        impl_routes = set(parse_coordinator_routes(f.read()).keys())

    documented = spec_route_keys(spec)
    rules = route_exclusions(spec)
    undocumented = sorted(
        (method, path)
        for method, path in impl_routes
        if (method, path) not in documented and not is_excluded(method, path, rules)
    )

    if undocumented:
        print(
            "[FAIL] Coordinator routes missing from the OpenAPI spec "
            "(add an operation or a documented x-route-exclusions rule):",
            file=sys.stderr,
        )
        for method, path in undocumented:
            print(f"  - {method.upper()} {path}", file=sys.stderr)
        return False

    excluded_count = sum(
        1
        for method, path in impl_routes
        if (method, path) not in documented and is_excluded(method, path, rules)
    )
    print(
        f"[OK] Route coverage validated: {len(impl_routes)} registered routes, "
        f"{len(documented & impl_routes)} documented, {excluded_count} documented exclusions."
    )
    return True


def validate_published_spec(spec: dict) -> bool:
    """Ensure the published docs/api spec is regenerated from the canonical spec."""
    import yaml  # PyYAML; load_spec() already verified availability

    if not PUBLISHED_SPEC_PATH.exists():
        print(f"[FAIL] Published API spec missing at {PUBLISHED_SPEC_PATH}", file=sys.stderr)
        return False
    with open(PUBLISHED_SPEC_PATH, encoding="utf-8") as f:
        published = yaml.safe_load(f)
    canonical_ops = spec_route_keys(spec)
    published_ops = spec_route_keys(published)
    if canonical_ops != published_ops:
        missing = sorted(canonical_ops - published_ops)
        stale = sorted(published_ops - canonical_ops)
        print("[FAIL] docs/api/openapi.yaml is out of sync with docs/openapi.yaml.", file=sys.stderr)
        for method, path in missing:
            print(f"  - missing in published spec: {method.upper()} {path}", file=sys.stderr)
        for method, path in stale:
            print(f"  - stale in published spec: {method.upper()} {path}", file=sys.stderr)
        print(
            "  Regenerate it with: cp docs/openapi.yaml docs/api/openapi.yaml",
            file=sys.stderr,
        )
        return False
    print("[OK] Published API spec matches the canonical spec.")
    return True


def run_schemathesis_offline() -> bool:
    """
    Use the schemathesis Python API to load the spec and confirm it parses
    without errors (exercising its own validation logic).
    """
    try:
        import schemathesis
    except ImportError:
        print(
            "WARNING: schemathesis not installed; skipping schema-loader check.\n"
            "         Install with: pip install schemathesis",
            file=sys.stderr,
        )
        return True  # non-blocking in offline mode

    loader_candidates = [
        ("schemathesis.from_path", getattr(schemathesis, "from_path", None)),
        (
            "schemathesis.openapi.from_path",
            getattr(getattr(schemathesis, "openapi", None), "from_path", None),
        ),
        (
            "schemathesis.openapi.from_file",
            getattr(getattr(schemathesis, "openapi", None), "from_file", None),
        ),
    ]

    loader_errors = []
    for loader_name, loader in loader_candidates:
        if not callable(loader):
            continue
        try:
            if loader_name.endswith("from_file"):
                with open(SPEC_PATH, "rb") as fd:
                    schema = loader(fd)
            else:
                schema = loader(str(SPEC_PATH))

            count = None
            if hasattr(schema, "get_all_operations"):
                count = sum(1 for _ in schema.get_all_operations())
            elif hasattr(schema, "get_all_endpoints"):
                count = sum(1 for _ in schema.get_all_endpoints())
            elif hasattr(schema, "__iter__"):
                count = sum(1 for _ in schema)

            if count is None:
                print(f"[OK] schemathesis loaded spec via {loader_name}.")
            else:
                print(f"[OK] schemathesis loaded spec via {loader_name}: {count} operations found.")
            return True
        except Exception as exc:
            loader_errors.append(f"{loader_name}: {exc}")

    # Fall back to the CLI if the Python API shape changed again. The CLI is
    # generally more stable across releases than the import surface.
    cli_cmd = [sys.executable, "-m", "schemathesis", "run", str(SPEC_PATH), "--dry-run"]
    result = subprocess.run(cli_cmd, capture_output=True, text=True)
    if result.returncode == 0:
        print("[OK] schemathesis loaded spec via CLI dry-run.")
        return True

    details = "; ".join(loader_errors) if loader_errors else "no compatible schemathesis loader found"
    cli_output = (result.stderr or result.stdout).strip()
    if cli_output:
        details = f"{details}; CLI dry-run: {cli_output}"
    print(f"[FAIL] schemathesis could not load spec: {details}", file=sys.stderr)
    return False


def run_schemathesis_live() -> bool:
    """
    Run a real schemathesis CLI conformance test against a live coordinator.
    Uses subprocess so the output streams directly to stdout/stderr.
    """
    checks = ",".join([
        "not_a_server_error",
        "response_schema_conformance",
        "status_code_conformance",
    ])

    # Endpoints that require auth or a running MPC stack are filtered out so
    # offline/CI runs against the coordinator in isolation still succeed.
    skip_operations = ",".join([
        "requestDeal",
        "requestReveal",
        "requestShowdown",
        "cancelMpcSession",
        "getPlayerCards",
        "playerAction",
        "createTable",
        "joinTable",
    ])

    cmd = [
        sys.executable, "-m", "schemathesis", "run",
        str(SPEC_PATH),
        "--base-url", COORDINATOR_URL,
        "--checks", checks,
        "--exclude-operations", skip_operations,
        "--validate-schema", "true",
        "--hypothesis-max-examples", "10",
        "--exitfirst",
    ]

    print(f"[INFO] Running: {' '.join(cmd)}")
    result = subprocess.run(cmd)
    return result.returncode == 0


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    if not SPEC_PATH.exists():
        print(f"ERROR: OpenAPI spec not found at {SPEC_PATH}", file=sys.stderr)
        return 1

    print(f"Coordinator API conformance check")
    print(f"  Spec : {SPEC_PATH}")
    print(f"  Mode : {'live — ' + COORDINATOR_URL if COORDINATOR_URL else 'offline'}")
    print()

    spec = load_spec()
    ok = validate_spec_structure(spec)
    ok = validate_handler_parity(spec) and ok
    ok = validate_route_coverage(spec) and ok
    ok = validate_published_spec(spec) and ok

    ok = run_schemathesis_offline() and ok

    if COORDINATOR_URL:
        ok = run_schemathesis_live() and ok

    print()
    if ok:
        print("PASS: All conformance checks passed.")
        return 0
    else:
        print("FAIL: One or more conformance checks failed.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
