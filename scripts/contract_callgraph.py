#!/usr/bin/env python3
"""
Contract function call-graph generator.

Parses the Soroban (Rust) smart contracts in this repo and produces a
call-graph diagram showing:

  * which functions call which (intra-contract call edges),
  * cross-contract calls (calls made through a generated `*Client`, e.g.
    `ZkVerifierClient`, `GameHubClient`, or the SAC `token::Client`),
  * authorization check points (`require_auth()`, `require_not_paused(...)`,
    `require_admin(...)` and similar guards).

This is a heuristic, regex/brace-matching parser rather than a full Rust
front-end -- it does not type-check, but it is good enough to map the public
entry points and call structure of Soroban contracts, which follow very
regular `#[contract]` / `#[contractimpl]` / `#[contractclient]` patterns.

Usage:
    scripts/contract_callgraph.py                      # all contracts -> Mermaid
    scripts/contract_callgraph.py --format dot         # Graphviz DOT
    scripts/contract_callgraph.py --format text        # plain-text report
    scripts/contract_callgraph.py -o docs/callgraph.md # write Markdown w/ diagram
    scripts/contract_callgraph.py --include-tests      # include test code
    scripts/contract_callgraph.py contracts/poker-table  # restrict to a path

Output format is chosen by --format (mermaid|dot|text|markdown). When -o ends
in .md, markdown is the default.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Set, Tuple

# --------------------------------------------------------------------------- #
# Patterns
# --------------------------------------------------------------------------- #

# A function definition (declaration or with a body). Captures the name.
FN_RE = re.compile(
    r"(?P<vis>pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:async\s+)?fn\s+(?P<name>\w+)\s*(?:<[^>]*>)?\s*\(",
)

# `let foo = SomethingClient::new(...)` -> binds a variable to a client type.
CLIENT_BIND_RE = re.compile(
    r"let\s+(?P<var>\w+)\s*=\s*(?P<ty>[\w:]+?Client)\s*::\s*new\b"
)
# `let token = token::Client::new(...)` -- the Stellar Asset Contract client.
TOKEN_BIND_RE = re.compile(r"let\s+(?P<var>\w+)\s*=\s*token::Client\s*::\s*new\b")

# `#[contractclient(name = "GameHubClient")]` over a `trait Foo { ... }`.
CONTRACTCLIENT_RE = re.compile(r'#\[contractclient\s*\(\s*name\s*=\s*"(?P<name>\w+)"')

# Authorization checkpoints.
AUTH_PATTERNS: List[Tuple[str, re.Pattern]] = [
    ("require_auth", re.compile(r"(?P<who>[\w.]+?)\s*\.\s*require_auth\s*\(")),
    ("require_not_paused", re.compile(r"\brequire_not_paused\s*\(")),
    ("require_admin", re.compile(r"\b(?:Self\s*::\s*)?require_admin\s*\(")),
]

# Rust keywords / macros that look like calls but are not function calls.
NOT_CALLS: Set[str] = {
    "if", "while", "for", "match", "return", "let", "fn", "loop", "else",
    "Ok", "Err", "Some", "None", "Vec", "Self", "assert", "assert_eq",
    "assert_ne", "panic", "vec", "matches", "format", "println", "Symbol",
    "BytesN", "Bytes", "Address", "Env", "as", "in", "move", "where",
}

CRATE_DIRS = ("committee-registry", "game-hub", "poker-table", "zk-verifier")


# --------------------------------------------------------------------------- #
# Model
# --------------------------------------------------------------------------- #


@dataclass
class CrossCall:
    client_type: str  # e.g. "ZkVerifierClient" or "token::Client"
    method: str       # e.g. "verify_deal"


@dataclass
class Func:
    name: str
    contract: str          # crate/contract name (directory)
    module: str            # file stem
    is_entrypoint: bool     # pub fn inside a #[contractimpl] block
    is_test: bool
    line: int
    body: str = ""
    calls: Set[str] = field(default_factory=set)        # resolved local fn names
    cross_calls: Set[Tuple[str, str]] = field(default_factory=set)  # (client, method)
    auth: List[str] = field(default_factory=list)        # human-readable checkpoints

    @property
    def qualified(self) -> str:
        return f"{self.contract}::{self.module}::{self.name}"

    @property
    def node_id(self) -> str:
        raw = f"{self.contract}_{self.module}_{self.name}"
        return re.sub(r"[^0-9A-Za-z_]", "_", raw)


@dataclass
class ContractModel:
    funcs: List[Func] = field(default_factory=list)
    # client type -> (external contract label, set of method names)
    clients: Dict[str, Tuple[str, Set[str]]] = field(default_factory=dict)


# --------------------------------------------------------------------------- #
# Brace / body extraction
# --------------------------------------------------------------------------- #


def _match_delim(src: str, open_pos: int, open_ch: str, close_ch: str) -> int:
    """Return index just past the delimiter that matches `src[open_pos]`.

    Skips delimiters that appear inside strings, chars and comments.
    """
    depth = 0
    i = open_pos
    n = len(src)
    while i < n:
        c = src[i]
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                if src[i] == "\\":
                    i += 1
                i += 1
            i += 1
            continue
        if c == "'":
            # Could be a char literal or a lifetime; only treat as a literal
            # when it looks like 'x' or '\n'.
            if i + 2 < n and (src[i + 2] == "'" or (src[i + 1] == "\\")):
                i += 1
                while i < n and src[i] != "'":
                    if src[i] == "\\":
                        i += 1
                    i += 1
                i += 1
                continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            i += 2
            while i + 1 < n and not (src[i] == "*" and src[i + 1] == "/"):
                i += 1
            i += 2
            continue
        if c == open_ch:
            depth += 1
        elif c == close_ch:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def _impl_block_ranges(src: str) -> List[Tuple[int, int, bool, bool]]:
    """Find `impl ... { }` blocks.

    Returns list of (start, end, is_contractimpl, is_cfg_test).
    """
    ranges = []
    for m in re.finditer(r"\bimpl\b", src):
        brace = src.find("{", m.start())
        if brace == -1:
            continue
        end = _match_delim(src, brace, "{", "}")
        # Look back ~200 chars for attributes on this impl.
        preamble = src[max(0, m.start() - 200):m.start()]
        is_contractimpl = "#[contractimpl]" in preamble
        is_cfg_test = "#[cfg(test)]" in preamble
        ranges.append((m.start(), end, is_contractimpl, is_cfg_test))
    return ranges


def _enclosing_impl(
    pos: int, impls: List[Tuple[int, int, bool, bool]]
) -> Optional[Tuple[int, int, bool, bool]]:
    best = None
    for r in impls:
        if r[0] <= pos < r[1]:
            if best is None or r[0] > best[0]:  # innermost
                best = r
    return best


def _cfg_test_mod_ranges(src: str) -> List[Tuple[int, int]]:
    """Ranges of `#[cfg(test)] mod foo { ... }` blocks."""
    ranges = []
    for m in re.finditer(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+", src):
        brace = src.find("{", m.end())
        if brace == -1:
            continue
        end = _match_delim(src, brace, "{", "}")
        ranges.append((m.start(), end))
    return ranges


# --------------------------------------------------------------------------- #
# Parsing
# --------------------------------------------------------------------------- #


def contract_name_for(path: str) -> str:
    parts = os.path.normpath(path).split(os.sep)
    for crate in CRATE_DIRS:
        if crate in parts:
            return crate
    # Fall back to the directory above src/.
    if "src" in parts:
        idx = parts.index("src")
        if idx > 0:
            return parts[idx - 1]
    return os.path.splitext(os.path.basename(path))[0]


def parse_file(path: str, model: ContractModel, include_tests: bool) -> None:
    with open(path, "r", encoding="utf-8") as fh:
        src = fh.read()

    contract = contract_name_for(path)
    module = os.path.splitext(os.path.basename(path))[0]
    file_is_test = "test" in module
    impls = _impl_block_ranges(src)
    test_mods = _cfg_test_mod_ranges(src)

    # Register cross-contract clients declared via #[contractclient].
    for m in CONTRACTCLIENT_RE.finditer(src):
        client_name = m.group("name")
        trait_m = re.search(r"\btrait\s+(\w+)", src[m.end():])
        label = trait_m.group(1) if trait_m else client_name
        # Collect declared method names from the trait body.
        methods: Set[str] = set()
        brace = src.find("{", m.end())
        if brace != -1:
            tend = _match_delim(src, brace, "{", "}")
            for fm in FN_RE.finditer(src[brace:tend]):
                methods.add(fm.group("name"))
        model.clients[client_name] = (label, methods)

    # Parse function definitions.
    for m in FN_RE.finditer(src):
        name = m.group("name")
        # Find params close paren, then locate body `{` or declaration `;`.
        paren_open = src.index("(", m.start())
        paren_end = _match_delim(src, paren_open, "(", ")")
        # Skip an optional return type up to the next `{` or `;`.
        rest = src[paren_end:]
        brace_rel = rest.find("{")
        semi_rel = rest.find(";")
        if semi_rel != -1 and (brace_rel == -1 or semi_rel < brace_rel):
            continue  # trait/extern declaration, no body
        if brace_rel == -1:
            continue
        body_start = paren_end + brace_rel
        body_end = _match_delim(src, body_start, "{", "}")
        body = src[body_start:body_end]

        impl = _enclosing_impl(m.start(), impls)
        is_pub = bool(m.group("vis"))
        is_entrypoint = bool(impl and impl[2] and is_pub)
        is_test = (
            file_is_test
            or "#[test]" in src[max(0, m.start() - 120):m.start()]
            or bool(impl and impl[3])
            or any(s <= m.start() < e for s, e in test_mods)
        )
        if is_test and not include_tests:
            continue

        line = src.count("\n", 0, m.start()) + 1
        model.funcs.append(
            Func(
                name=name,
                contract=contract,
                module=module,
                is_entrypoint=is_entrypoint,
                is_test=is_test,
                line=line,
                body=body,
            )
        )


def analyze(model: ContractModel) -> None:
    """Resolve call edges, cross-contract calls and auth checkpoints."""
    # Index defined function names per contract for local resolution.
    by_contract: Dict[str, Dict[str, Func]] = {}
    for fn in model.funcs:
        by_contract.setdefault(fn.contract, {})[fn.name] = fn

    for fn in model.funcs:
        body = fn.body
        local = by_contract.get(fn.contract, {})

        # --- client variable bindings within this body --------------------- #
        var_to_client: Dict[str, str] = {}
        for cm in CLIENT_BIND_RE.finditer(body):
            var_to_client[cm.group("var")] = cm.group("ty").split("::")[-1]
        for tm in TOKEN_BIND_RE.finditer(body):
            var_to_client[tm.group("var")] = "token::Client"

        # --- cross-contract calls: <clientvar>.<method>( ------------------- #
        for var, client_ty in var_to_client.items():
            for cc in re.finditer(rf"\b{re.escape(var)}\s*\.\s*(\w+)\s*\(", body):
                method = cc.group(1)
                if method in ("new",):
                    continue
                fn.cross_calls.add((client_ty, method))

        # --- authorization checkpoints ------------------------------------- #
        for label, pat in AUTH_PATTERNS:
            for am in pat.finditer(body):
                if label == "require_auth":
                    who = am.group("who").strip()
                    fn.auth.append(f"{who}.require_auth()")
                else:
                    fn.auth.append(f"{label}()")

        # --- intra-contract call edges ------------------------------------- #
        # Bare `name(`, `Self::name(`, and `module::name(`.
        for cm in re.finditer(r"(?:(\w+)\s*::\s*)?(\w+)\s*\(", body):
            qualifier, callee = cm.group(1), cm.group(2)
            if callee == fn.name and qualifier in (None, "Self"):
                # Likely recursion or the definition echo; allow recursion.
                pass
            if callee in NOT_CALLS:
                continue
            if qualifier and qualifier not in (None, "Self", fn.module) and \
                    qualifier not in by_contract.get(fn.contract, {}):
                # Qualified by a module name; only resolve if it maps to a local fn.
                pass
            if callee in local and callee != fn.name:
                fn.calls.add(callee)
            elif callee in local and callee == fn.name:
                fn.calls.add(callee)  # recursion


# --------------------------------------------------------------------------- #
# Rendering
# --------------------------------------------------------------------------- #


def _client_label(model: ContractModel, client_ty: str) -> str:
    if client_ty == "token::Client":
        return "Token SAC"
    base = model.clients.get(client_ty)
    if base:
        return base[0]
    return client_ty.replace("Client", "")


def render_mermaid(model: ContractModel) -> str:
    out: List[str] = ["flowchart LR"]
    funcs = sorted(model.funcs, key=lambda f: (f.contract, f.module, f.name))

    # Group functions into subgraphs per contract.
    contracts: Dict[str, List[Func]] = {}
    for fn in funcs:
        contracts.setdefault(fn.contract, []).append(fn)

    node_index = {fn.qualified: fn for fn in funcs}

    for contract, fns in contracts.items():
        out.append(f'  subgraph {re.sub(r"[^0-9A-Za-z_]", "_", contract)}["📜 {contract}"]')
        for fn in fns:
            marker = "🔒 " if fn.auth else ""
            shape_l, shape_r = ("([", "])") if fn.is_entrypoint else ("[", "]")
            label = f"{marker}{fn.module}::{fn.name}"
            out.append(f'    {fn.node_id}{shape_l}"{label}"{shape_r}')
        out.append("  end")

    # External contract nodes (cross-call targets).
    externals: Set[str] = set()
    for fn in funcs:
        for client_ty, _method in fn.cross_calls:
            externals.add(_client_label(model, client_ty))
    for ext in sorted(externals):
        ext_id = "ext_" + re.sub(r"[^0-9A-Za-z_]", "_", ext)
        out.append(f'  {ext_id}{{{{"🌐 {ext}"}}}}')

    # Intra-contract edges.
    name_to_fn: Dict[Tuple[str, str], Func] = {}
    for fn in funcs:
        name_to_fn[(fn.contract, fn.name)] = fn
    for fn in funcs:
        for callee in sorted(fn.calls):
            target = name_to_fn.get((fn.contract, callee))
            if target and target is not fn:
                out.append(f"  {fn.node_id} --> {target.node_id}")
            elif target is fn:
                out.append(f"  {fn.node_id} -.->|recurses| {fn.node_id}")

    # Cross-contract edges (dashed).
    for fn in funcs:
        for client_ty, method in sorted(fn.cross_calls):
            ext = _client_label(model, client_ty)
            ext_id = "ext_" + re.sub(r"[^0-9A-Za-z_]", "_", ext)
            out.append(f"  {fn.node_id} -.->|{method}| {ext_id}")

    out.append("")
    out.append("  classDef entry fill:#dff0d8,stroke:#3c763d;")
    out.append("  classDef ext fill:#fcf8e3,stroke:#8a6d3b;")
    entry_ids = [fn.node_id for fn in funcs if fn.is_entrypoint]
    if entry_ids:
        out.append("  class " + ",".join(entry_ids) + " entry;")
    if externals:
        ext_ids = ["ext_" + re.sub(r"[^0-9A-Za-z_]", "_", e) for e in sorted(externals)]
        out.append("  class " + ",".join(ext_ids) + " ext;")
    return "\n".join(out)


def render_dot(model: ContractModel) -> str:
    out: List[str] = ["digraph callgraph {", "  rankdir=LR;", '  node [fontname="monospace"];']
    funcs = sorted(model.funcs, key=lambda f: (f.contract, f.module, f.name))

    contracts: Dict[str, List[Func]] = {}
    for fn in funcs:
        contracts.setdefault(fn.contract, []).append(fn)

    for contract, fns in contracts.items():
        cid = re.sub(r"[^0-9A-Za-z_]", "_", contract)
        out.append(f'  subgraph cluster_{cid} {{')
        out.append(f'    label="{contract}"; style=filled; color="#eeeeee";')
        for fn in fns:
            shape = "box" if not fn.is_entrypoint else "box"
            style = "filled" if fn.is_entrypoint else "solid"
            fill = "#dff0d8" if fn.is_entrypoint else "white"
            marker = "🔒 " if fn.auth else ""
            label = f"{marker}{fn.module}::{fn.name}"
            out.append(
                f'    {fn.node_id} [label="{label}", shape={shape}, '
                f'style="{style}", fillcolor="{fill}"];'
            )
        out.append("  }")

    externals: Set[str] = set()
    for fn in funcs:
        for client_ty, _ in fn.cross_calls:
            externals.add(_client_label(model, client_ty))
    for ext in sorted(externals):
        ext_id = "ext_" + re.sub(r"[^0-9A-Za-z_]", "_", ext)
        out.append(f'  {ext_id} [label="🌐 {ext}", shape=hexagon, style=filled, fillcolor="#fcf8e3"];')

    name_to_fn = {(fn.contract, fn.name): fn for fn in funcs}
    for fn in funcs:
        for callee in sorted(fn.calls):
            target = name_to_fn.get((fn.contract, callee))
            if target:
                out.append(f"  {fn.node_id} -> {target.node_id};")
    for fn in funcs:
        for client_ty, method in sorted(fn.cross_calls):
            ext = _client_label(model, client_ty)
            ext_id = "ext_" + re.sub(r"[^0-9A-Za-z_]", "_", ext)
            out.append(f'  {fn.node_id} -> {ext_id} [style=dashed, label="{method}"];')

    out.append("}")
    return "\n".join(out)


def render_text(model: ContractModel) -> str:
    out: List[str] = []
    funcs = sorted(model.funcs, key=lambda f: (f.contract, f.module, f.name))
    contracts: Dict[str, List[Func]] = {}
    for fn in funcs:
        contracts.setdefault(fn.contract, []).append(fn)

    name_to_fn = {(fn.contract, fn.name): fn for fn in funcs}

    for contract, fns in contracts.items():
        out.append(f"\n=== Contract: {contract} ===")
        for fn in fns:
            tags = []
            if fn.is_entrypoint:
                tags.append("entrypoint")
            if fn.auth:
                tags.append("auth")
            tag_str = f"  [{', '.join(tags)}]" if tags else ""
            out.append(f"\n  {fn.module}::{fn.name}{tag_str}  (line {fn.line})")
            if fn.auth:
                for a in fn.auth:
                    out.append(f"      🔒 {a}")
            for callee in sorted(fn.calls):
                if (fn.contract, callee) in name_to_fn:
                    out.append(f"      -> {callee}()")
            for client_ty, method in sorted(fn.cross_calls):
                ext = _client_label(model, client_ty)
                out.append(f"      ⇒ [cross-contract] {ext}.{method}()")
    return "\n".join(out).lstrip("\n")


def render_auth_table(model: ContractModel) -> str:
    rows = []
    for fn in sorted(model.funcs, key=lambda f: (f.contract, f.module, f.name)):
        if not fn.is_entrypoint and not fn.auth:
            continue
        checks = "; ".join(dict.fromkeys(fn.auth)) if fn.auth else "—"
        kind = "entry" if fn.is_entrypoint else "fn"
        rows.append(f"| {fn.contract} | `{fn.module}::{fn.name}` | {kind} | {checks} |")
    header = (
        "| Contract | Function | Kind | Authorization checkpoints |\n"
        "|---|---|---|---|"
    )
    return header + "\n" + "\n".join(rows)


def render_markdown(model: ContractModel) -> str:
    n_funcs = len(model.funcs)
    n_entries = sum(1 for f in model.funcs if f.is_entrypoint)
    n_cross = sum(len(f.cross_calls) for f in model.funcs)
    n_auth = sum(1 for f in model.funcs if f.auth)
    parts = [
        "# Contract Call-Graph",
        "",
        "_Generated by `scripts/contract_callgraph.py`._",
        "",
        f"- Functions analyzed: **{n_funcs}**",
        f"- Public entry points (`#[contractimpl]`): **{n_entries}**",
        f"- Functions with auth checkpoints: **{n_auth}**",
        f"- Cross-contract call sites: **{n_cross}**",
        "",
        "Legend: `([rounded])` = public entry point, `🔒` = has an authorization "
        "checkpoint, `🌐` = external contract reached via a generated client, "
        "dashed edge = cross-contract call.",
        "",
        "## Diagram",
        "",
        "```mermaid",
        render_mermaid(model),
        "```",
        "",
        "## Authorization checkpoints",
        "",
        render_auth_table(model),
        "",
        "## Call detail",
        "",
        "```",
        render_text(model),
        "```",
        "",
    ]
    return "\n".join(parts)


# --------------------------------------------------------------------------- #
# Driver
# --------------------------------------------------------------------------- #


def collect_rust_files(roots: List[str]) -> List[str]:
    files: List[str] = []
    for root in roots:
        if os.path.isfile(root) and root.endswith(".rs"):
            files.append(root)
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            if "target" in dirnames:
                dirnames.remove("target")
            for fn in filenames:
                if fn.endswith(".rs"):
                    files.append(os.path.join(dirpath, fn))
    return sorted(files)


def main(argv: Optional[List[str]] = None) -> int:
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    default_root = os.path.join(repo_root, "contracts")

    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument(
        "paths", nargs="*", default=[default_root],
        help="Contract dirs or .rs files to scan (default: contracts/).",
    )
    ap.add_argument(
        "-f", "--format", choices=["mermaid", "dot", "text", "markdown"],
        help="Output format (default: markdown if -o ends in .md, else mermaid).",
    )
    ap.add_argument("-o", "--output", help="Write to file instead of stdout.")
    ap.add_argument(
        "--include-tests", action="store_true",
        help="Include test functions / mock impls (excluded by default).",
    )
    args = ap.parse_args(argv)

    fmt = args.format
    if fmt is None:
        fmt = "markdown" if (args.output and args.output.endswith(".md")) else "mermaid"

    files = collect_rust_files(args.paths)
    if not files:
        print("No .rs files found.", file=sys.stderr)
        return 1

    model = ContractModel()
    for path in files:
        try:
            parse_file(path, model, args.include_tests)
        except Exception as exc:  # pragma: no cover - defensive
            print(f"warning: failed to parse {path}: {exc}", file=sys.stderr)
    analyze(model)

    if not model.funcs:
        print("No functions found.", file=sys.stderr)
        return 1

    renderers = {
        "mermaid": render_mermaid,
        "dot": render_dot,
        "text": render_text,
        "markdown": render_markdown,
    }
    output = renderers[fmt](model)

    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(output + "\n")
        print(
            f"Wrote {fmt} call-graph for {len(model.funcs)} functions to {args.output}",
            file=sys.stderr,
        )
    else:
        print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
