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
EXCLUSIONS_PATH = (
    pathlib.Path(__file__).parent.parent / "docs" / "api" / "route-exclusions.yaml"
)
COORDINATOR_URL = os.environ.get("COORDINATOR_URL", "")
HTTP_METHODS = ("get", "post", "put", "patch", "delete")

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


def to_openapi_path(path: str) -> str:
    """Convert an Axum `:param` path into the `{param}` form used by OpenAPI."""
    return re.sub(r":([A-Za-z0-9_]+)", r"{\1}", path)


def format_route(route) -> str:
    method, path = route
    return f"{method.upper()} {path}"


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
                for method in HTTP_METHODS:
                    if re.search(r'(?:axum::routing::|\b)' + method + r'\(', handler_expr) or re.search(r'\.' + method + r'\(', handler_expr):
                        routes[(method, to_openapi_path(path))] = handler_expr
        idx = pos + 7
    return routes


def load_route_exclusions():
    """
    Load and validate docs/api/route-exclusions.yaml.

    Returns (set_of_(method, path), category_rationales), or None when the
    manifest is missing/malformed.
    """
    try:
        import yaml  # PyYAML
    except ImportError:
        print("ERROR: PyYAML not installed. Run: pip install pyyaml", file=sys.stderr)
        return None

    if not EXCLUSIONS_PATH.exists():
        print(
            f"[FAIL] Route exclusion manifest missing at {EXCLUSIONS_PATH}",
            file=sys.stderr,
        )
        return None

    with open(EXCLUSIONS_PATH, encoding="utf-8") as f:
        doc = yaml.safe_load(f)

    errors = []
    if not isinstance(doc, dict):
        print("[FAIL] Route exclusion manifest must be a YAML mapping.", file=sys.stderr)
        return None

    if doc.get("version") != 1:
        errors.append("version must be 1")

    categories = doc.get("categories")
    routes = doc.get("routes")
    if not isinstance(categories, dict) or not categories:
        errors.append("categories must be a non-empty mapping of category -> rationale")
        categories = {}
    if not isinstance(routes, list):
        errors.append("routes must be a list")
        routes = []

    for category, rationale in categories.items():
        if not isinstance(category, str) or not category.strip():
            errors.append("category names must be non-empty strings")
        if not isinstance(rationale, str) or not rationale.strip():
            errors.append(f"category '{category}' has no written rationale")

    excluded = set()
    seen = set()
    for entry in routes:
        if not isinstance(entry, dict):
            errors.append(f"invalid exclusion entry: {entry!r}")
            continue
        method = str(entry.get("method", "")).strip().lower()
        path = entry.get("path")
        category = entry.get("category")
        handler = entry.get("handler", "<unknown>")

        if method not in HTTP_METHODS:
            errors.append(f"invalid HTTP method '{method}' for {handler}")
            continue
        if not isinstance(path, str) or not path.startswith("/"):
            errors.append(f"invalid path {path!r} for {handler}")
            continue
        if category not in categories:
            errors.append(f"exclusion {method.upper()} {path} uses unknown category '{category}'")

        key = (method, to_openapi_path(path))
        if key in seen:
            errors.append(f"duplicate exclusion: {format_route(key)}")
        seen.add(key)
        excluded.add(key)

    if errors:
        for error in errors:
            print(f"[EXCLUSION ERROR] {error}", file=sys.stderr)
        return None

    print(
        f"[OK] Route exclusion manifest loaded: "
        f"{len(categories)} categories, {len(excluded)} excluded operations."
    )
    return excluded, categories


def validate_handler_parity(spec: dict) -> bool:
    """
    Assert zero drift in both directions between OpenAPI operations and the
    coordinator's production Axum router.

    A registered route must either be present in docs/openapi.yaml or be
    listed in docs/api/route-exclusions.yaml with a written rationale.
    Stale spec entries and stale exclusions are also rejected.
    """
    if not MAIN_RS_PATH.exists():
        print(f"[WARN] Coordinator main.rs not found at {MAIN_RS_PATH}; skipping parity check.")
        return True

    exclusions = load_route_exclusions()
    if exclusions is None:
        return False
    excluded_routes, _categories = exclusions

    with open(MAIN_RS_PATH, encoding="utf-8") as f:
        main_content = f.read()

    impl_routes = set(parse_coordinator_routes(main_content))
    spec_routes = {}
    for path, path_item in spec.get("paths", {}).items():
        for method, op in path_item.items():
            if method.lower() in HTTP_METHODS:
                spec_routes[(method.lower(), path)] = op.get("operationId", "unnamed")

    spec_keys = set(spec_routes)
    failures = []

    missing_in_impl = sorted(
        route for route in spec_keys if route not in impl_routes
    )
    if missing_in_impl:
        failures.append("OpenAPI operations missing from coordinator router:")
        for route in missing_in_impl:
            failures.append(
                f"  - {format_route(route)} (operationId: {spec_routes[route]})"
            )

    undocumented = sorted(impl_routes - spec_keys - excluded_routes)
    if undocumented:
        failures.append(
            "Coordinator routes missing from the spec and from the exclusion manifest:"
        )
        for route in undocumented:
            failures.append(f"  - {format_route(route)}")

    stale_spec = sorted(spec_keys - impl_routes)
    if stale_spec:
        failures.append("OpenAPI operations whose router registration no longer exists:")
        for route in stale_spec:
            failures.append(f"  - {format_route(route)}")

    stale_exclusions = sorted(excluded_routes - impl_routes)
    if stale_exclusions:
        failures.append("Stale exclusions whose router registration no longer exists:")
        for route in stale_exclusions:
            failures.append(f"  - {format_route(route)}")

    contradictions = sorted(excluded_routes & spec_keys)
    if contradictions:
        failures.append("Routes listed both in the spec and in the exclusion manifest:")
        for route in contradictions:
            failures.append(f"  - {format_route(route)}")

    if failures:
        print("[FAIL] Router/spec parity check failed:", file=sys.stderr)
        for line in failures:
            print(line, file=sys.stderr)
        return False

    print(
        f"[OK] Handler parity validated: {len(spec_routes)} documented operations, "
        f"{len(excluded_routes)} documented exclusions, "
        f"{len(impl_routes)} registered operations, zero drift."
    )
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
    print(f"  Mode : {'live - ' + COORDINATOR_URL if COORDINATOR_URL else 'offline'}")
    print()

    spec = load_spec()
    ok = validate_spec_structure(spec)
    ok = validate_handler_parity(spec) and ok

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