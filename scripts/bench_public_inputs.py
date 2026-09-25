#!/usr/bin/env python3
"""Benchmark proof size and verification cost against public-input count (#526).

On-chain cost has two parts that depend on the public inputs a circuit
exposes: the bytes submitted with the transaction (proof + public inputs) and
the verifier work that scales with their number. This script measures both
on synthetic circuits that expose K public field elements, then places the
real hand circuits (deal_valid, reveal_board_valid, showdown_valid) on the
same curve using the public-input count read from their compiled ABI.

Measurements per K:
  - ACIR opcodes                 nargo info --json
  - UltraHonk circuit size       bb gates                (when bb is on PATH)
  - proof bytes, public bytes    bb prove                (when bb is on PATH)
  - verify wall time             bb verify, median of 3  (when bb is on PATH)

Without bb the byte columns fall back to the documented UltraHonk sizes
(16 256-byte proof, 32 bytes per public field) and are marked as such.

Outputs (all regenerated in place):
  circuits/BENCHMARKS.md         the section between the public-inputs markers
  benchmark_data/public_inputs.csv
  benchmark_data/public_inputs.svg

Usage:
  python3 scripts/bench_public_inputs.py             # full run
  python3 scripts/bench_public_inputs.py --no-bb     # nargo only
  python3 scripts/bench_public_inputs.py --sweep 1,4,16,64
"""

from __future__ import annotations

import argparse
import csv
import json
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CIRCUITS = ("deal_valid", "reveal_board_valid", "showdown_valid")
DEFAULT_SWEEP = (1, 2, 4, 8, 16, 32, 64, 128, 256)
FIELD_BYTES = 32
DOC_PROOF_BYTES = 16_256  # UltraHonk proof size documented in BENCHMARKS.md
START = "<!-- public-inputs-benchmark:start -->"
END = "<!-- public-inputs-benchmark:end -->"


def run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed:\n{proc.stderr or proc.stdout}")
    return proc


def nargo_version() -> str:
    return run(["nargo", "--version"]).stdout.splitlines()[0].split("=")[-1].strip()


def acir_opcodes(program_dir: Path) -> int:
    info = json.loads(run(["nargo", "info", "--json", "--program-dir", str(program_dir)]).stdout)
    for fn in info["programs"][0]["functions"]:
        if fn["name"] == "main":
            return int(fn["opcodes"])
    raise RuntimeError(f"no main function in nargo info for {program_dir}")


def bb_circuit_size(bytecode: Path) -> int | None:
    if shutil.which("bb") is None:
        return None
    out = run(["bb", "gates", "--scheme", "ultra_honk", "-b", str(bytecode)]).stdout
    return int(json.loads(out)["functions"][0]["circuit_size"])


def abi_size(abi_type: dict) -> int:
    kind = abi_type["kind"]
    if kind in ("field", "integer", "boolean"):
        return 1
    if kind == "array":
        return int(abi_type["length"]) * abi_size(abi_type["type"])
    if kind == "string":
        return int(abi_type["length"])
    if kind == "tuple":
        return sum(abi_size(t) for t in abi_type["fields"])
    if kind == "struct":
        return sum(abi_size(f["type"]) for f in abi_type["fields"])
    raise RuntimeError(f"unknown ABI type kind {kind}")


def public_field_count(artifact: Path) -> tuple[int, int]:
    """(public parameters, public return fields) from a compiled artifact's ABI."""
    abi = json.loads(artifact.read_text())["abi"]
    params = sum(abi_size(p["type"]) for p in abi["parameters"] if p["visibility"] == "public")
    ret = abi.get("return_type")
    returns = abi_size(ret["abi_type"]) if ret and ret["visibility"] == "public" else 0
    return params, returns


def synthetic_project(k: int, workdir: Path) -> Path:
    project = workdir / f"pub_inputs_{k}"
    (project / "src").mkdir(parents=True)
    (project / "Nargo.toml").write_text(
        '[package]\nname = "pub_inputs"\ntype = "bin"\ncompiler_version = ">=0.36.0"\n\n[dependencies]\n'
    )
    # One private witness and K public inputs, all of which the constraint
    # system has to touch, so no input is optimised away.
    (project / "src" / "main.nr").write_text(
        f"global K: u32 = {k};\n"
        "fn main(secret: Field, inputs: pub [Field; K]) -> pub Field {\n"
        "    let mut acc = secret;\n"
        "    for i in 0..K {\n"
        "        acc = acc * inputs[i] + inputs[i];\n"
        "    }\n"
        "    acc\n"
        "}\n"
    )
    inputs = ", ".join(f'"{i + 1}"' for i in range(k))
    (project / "Prover.toml").write_text(f'secret = "7"\ninputs = [{inputs}]\n')
    return project


def measure_synthetic(k: int, workdir: Path, use_bb: bool) -> dict:
    project = synthetic_project(k, workdir)
    run(["nargo", "compile", "--program-dir", str(project)])
    row = {
        "public_fields": k + 1,  # K inputs plus the public return value
        "acir_opcodes": acir_opcodes(project),
        "circuit_size": None,
        "proof_bytes": DOC_PROOF_BYTES,
        "public_bytes": (k + 1) * FIELD_BYTES,
        "verify_ms": None,
        "measured": False,
    }
    if not use_bb:
        return row
    bytecode = project / "target" / "pub_inputs.json"
    witness = project / "target" / "pub_inputs.gz"
    out = project / "out"
    run(["nargo", "execute", "--program-dir", str(project)])
    run(["bb", "write_vk", "--scheme", "ultra_honk", "-b", str(bytecode), "-o", str(out)])
    run([
        "bb", "prove", "--scheme", "ultra_honk", "-b", str(bytecode), "-w", str(witness),
        "-k", str(out / "vk"), "-o", str(out),
    ])
    timings = []
    for _ in range(3):
        started = time.perf_counter()
        run([
            "bb", "verify", "--scheme", "ultra_honk", "-k", str(out / "vk"),
            "-p", str(out / "proof"), "-i", str(out / "public_inputs"),
        ])
        timings.append((time.perf_counter() - started) * 1000)
    row.update(
        circuit_size=bb_circuit_size(bytecode),
        proof_bytes=(out / "proof").stat().st_size,
        public_bytes=(out / "public_inputs").stat().st_size,
        verify_ms=round(statistics.median(timings), 1),
        measured=True,
    )
    return row


def measure_circuit(name: str, use_bb: bool) -> dict:
    program_dir = ROOT / "circuits" / name
    run(["nargo", "compile", "--program-dir", str(program_dir)])
    artifact = program_dir / "target" / f"{name}.json"
    params, returns = public_field_count(artifact)
    total = params + returns
    return {
        "circuit": name,
        "public_params": params,
        "public_returns": returns,
        "public_fields": total,
        "public_bytes": total * FIELD_BYTES,
        "acir_opcodes": acir_opcodes(program_dir),
        "circuit_size": bb_circuit_size(artifact) if use_bb else None,
    }


def fmt(value) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.1f}"
    return f"{value:,}"


def render_section(sweep: list[dict], circuits: list[dict], meta: dict) -> str:
    lines = [
        START,
        "## Proof Size vs Public-Input Count",
        "",
        f"_Generated by `scripts/bench_public_inputs.py` with Noir `{meta['nargo']}`"
        + (f" and bb `{meta['bb']}`" if meta["bb"] else " (bb not available; byte columns use the documented UltraHonk sizes)")
        + ". Re-run the script to refresh; do not edit by hand._",
        "",
        "A public input costs 32 bytes on the wire and one extra term in the",
        "verifier's public-input polynomial evaluation. The UltraHonk proof itself",
        "does not grow with the number of public inputs, so the marginal on-chain",
        "cost of exposing one more field element is those 32 bytes plus that",
        "evaluation term. Measured on synthetic circuits that expose K public",
        "inputs and one public output:",
        "",
        "| Public fields | ACIR opcodes | UltraHonk gates | Proof bytes | Public-input bytes | Total bytes | Verify (ms) |",
        "| ------------: | -----------: | --------------: | ----------: | -----------------: | ----------: | ----------: |",
    ]
    for r in sweep:
        total = r["proof_bytes"] + r["public_bytes"]
        lines.append(
            f"| {fmt(r['public_fields'])} | {fmt(r['acir_opcodes'])} | {fmt(r['circuit_size'])} | "
            f"{fmt(r['proof_bytes'])} | {fmt(r['public_bytes'])} | {fmt(total)} | {fmt(r['verify_ms'])} |"
        )
    lines += [
        "",
        "The hand circuits placed on that curve, with counts read from each",
        "compiled artifact's ABI (public parameters plus public return values):",
        "",
        "| Circuit | Public params | Public returns | Public fields | Public-input bytes | Total bytes | ACIR opcodes | UltraHonk gates |",
        "| ------- | ------------: | -------------: | ------------: | -----------------: | ----------: | -----------: | --------------: |",
    ]
    proof_bytes = sweep[0]["proof_bytes"] if sweep else DOC_PROOF_BYTES
    for c in circuits:
        lines.append(
            f"| `{c['circuit']}` | {fmt(c['public_params'])} | {fmt(c['public_returns'])} | {fmt(c['public_fields'])} | "
            f"{fmt(c['public_bytes'])} | {fmt(proof_bytes + c['public_bytes'])} | {fmt(c['acir_opcodes'])} | {fmt(c['circuit_size'])} |"
        )
    lines += [
        "",
        "Reading the curve: every hand circuit sits in the flat part, where the",
        "proof dominates and public inputs are under 5% of the bytes. Halving a",
        "circuit's public inputs saves a few hundred bytes per proof; folding many",
        "hands behind one digest (see docs/recursive-public-input-packing.md) is",
        "what changes the picture, because it removes whole proofs, not fields.",
        "",
        "![Proof bytes against public-input count](../benchmark_data/public_inputs.svg)",
        "",
        "Raw data: [`benchmark_data/public_inputs.csv`](../benchmark_data/public_inputs.csv).",
        END,
    ]
    return "\n".join(lines)


def write_svg(sweep: list[dict], path: Path) -> None:
    width, height, pad = 720, 360, 56
    xs = [r["public_fields"] for r in sweep]
    proof = [r["proof_bytes"] for r in sweep]
    total = [r["proof_bytes"] + r["public_bytes"] for r in sweep]
    x_max = max(xs)
    y_max = max(total) * 1.05

    def px(x):
        return pad + (width - 2 * pad) * (x / x_max)

    def py(y):
        return height - pad - (height - 2 * pad) * (y / y_max)

    def polyline(values, colour):
        points = " ".join(f"{px(x):.1f},{py(y):.1f}" for x, y in zip(xs, values))
        return f'<polyline fill="none" stroke="{colour}" stroke-width="2" points="{points}" />'

    ticks = "".join(
        f'<text x="{px(x):.1f}" y="{height - pad + 18}" font-size="11" text-anchor="middle">{x}</text>'
        for x in xs
    )
    y_ticks = "".join(
        f'<text x="{pad - 6}" y="{py(y):.1f}" font-size="11" text-anchor="end">{y:,}</text>'
        for y in (0, DOC_PROOF_BYTES, int(y_max))
    )
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" font-family="sans-serif">'
        f'<rect width="{width}" height="{height}" fill="white" />'
        f'<line x1="{pad}" y1="{height - pad}" x2="{width - pad}" y2="{height - pad}" stroke="#444" />'
        f'<line x1="{pad}" y1="{pad}" x2="{pad}" y2="{height - pad}" stroke="#444" />'
        f"{polyline(proof, '#888')}{polyline(total, '#1f6feb')}{ticks}{y_ticks}"
        f'<text x="{width / 2}" y="{height - 12}" font-size="12" text-anchor="middle">public field elements</text>'
        f'<text x="{pad + 8}" y="{pad - 8}" font-size="12">bytes on the wire (grey: proof only, blue: proof + public inputs)</text>'
        "</svg>\n"
    )
    path.write_text(svg)


def update_benchmarks_md(section: str) -> None:
    doc = ROOT / "circuits" / "BENCHMARKS.md"
    text = doc.read_text()
    if START in text and END in text:
        before = text[: text.index(START)]
        after = text[text.index(END) + len(END):]
        text = before + section + after
    else:
        anchor = "## Circuit Descriptions"
        idx = text.index(anchor)
        text = text[:idx] + section + "\n\n---\n\n" + text[idx:]
    doc.write_text(text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sweep", default=",".join(map(str, DEFAULT_SWEEP)), help="comma-separated public-input counts")
    parser.add_argument("--no-bb", action="store_true", help="skip bb even if it is installed")
    args = parser.parse_args()

    if shutil.which("nargo") is None:
        print("nargo is not on PATH", file=sys.stderr)
        return 1
    use_bb = not args.no_bb and shutil.which("bb") is not None
    meta = {"nargo": nargo_version(), "bb": run(["bb", "--version"]).stdout.strip() if use_bb else None}
    sweep_values = [int(v) for v in args.sweep.split(",") if v.strip()]

    sweep = []
    with tempfile.TemporaryDirectory(prefix="stellpoker-pubinputs-") as tmp:
        for k in sweep_values:
            row = measure_synthetic(k, Path(tmp), use_bb)
            sweep.append(row)
            print(f"  K={k:>4}: {row['public_bytes']:>6} public bytes, proof {row['proof_bytes']} bytes, "
                  f"{row['acir_opcodes']} ACIR opcodes" + (f", verify {row['verify_ms']} ms" if row['verify_ms'] else ""))

    circuits = [measure_circuit(name, use_bb) for name in CIRCUITS]
    for c in circuits:
        print(f"  {c['circuit']}: {c['public_fields']} public fields ({c['public_bytes']} bytes), {c['acir_opcodes']} ACIR opcodes")

    data_dir = ROOT / "benchmark_data"
    data_dir.mkdir(exist_ok=True)
    with (data_dir / "public_inputs.csv").open("w", newline="") as fh:
        writer = csv.writer(fh, lineterminator="\n")
        writer.writerow(["kind", "name", "public_fields", "acir_opcodes", "circuit_size", "proof_bytes", "public_bytes", "total_bytes", "verify_ms", "measured"])
        for r in sweep:
            writer.writerow(["synthetic", f"K={r['public_fields'] - 1}", r["public_fields"], r["acir_opcodes"], r["circuit_size"] or "", r["proof_bytes"], r["public_bytes"], r["proof_bytes"] + r["public_bytes"], r["verify_ms"] or "", r["measured"]])
        proof_bytes = sweep[0]["proof_bytes"] if sweep else DOC_PROOF_BYTES
        for c in circuits:
            writer.writerow(["circuit", c["circuit"], c["public_fields"], c["acir_opcodes"], c["circuit_size"] or "", proof_bytes, c["public_bytes"], proof_bytes + c["public_bytes"], "", bool(c["circuit_size"])])
    write_svg(sweep, data_dir / "public_inputs.svg")
    update_benchmarks_md(render_section(sweep, circuits, meta))
    print("updated circuits/BENCHMARKS.md, benchmark_data/public_inputs.csv, benchmark_data/public_inputs.svg")
    return 0


if __name__ == "__main__":
    sys.exit(main())
