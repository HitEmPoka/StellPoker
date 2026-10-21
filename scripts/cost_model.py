#!/usr/bin/env python3
"""Operator cost model for StellPoker: XLM fees + MPC compute per hand (#586).

Committee fee distribution pays nodes in proportion to stake, but an operator
needs to know, before running a table, roughly what a hand *costs* to serve.
This calculator estimates two things:

1. **On-chain fees** paid in XLM for the Soroban transactions a hand
   requires — transaction base fees plus the resource fee for CPU
   instructions, disk reads and writes.
2. **MPC compute cost** — the co-noir witness + UltraHonk proof generation
   each hand needs, expressed as node-seconds and converted to XLM (or USD)
   with an operator-supplied hourly node cost.

All inputs are estimates that depend on the live network: review
`docs/cost-model.md` before trusting the defaults, and prefer overriding them
with values measured on your own deployment (see
`scripts/collect_percentile_benchmarks.py` and `scripts/bench_circuit_gas.py`).

Usage:
    python3 scripts/cost_model.py --hands-per-day 10000
    python3 scripts/cost_model.py --hands-per-day 50000 --proof-bytes 14624 \\
        --rpc-cost-xlm 0.00001 --node-hourly-cost-xlm 2.0 --json
    python3 scripts/cost_model.py --scenario-priority --hands-per-day 2000

Defaults are drawn from the project's own benchmarks: proof size 14 624 bytes
and per-circuit backend opcode counts (README "Circuit Benchmarking").
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import asdict, dataclass, field

# 1 XLM = 10^7 stroops. All fee arithmetic is done in stroops.
STROOPS_PER_XLM = 10_000_000

# Backend opcode counts per circuit (README "Circuit Benchmarking", 2-6
# players). These are the UltraHonk gate counts a hand's proofs consume.
CIRCUIT_CPU_INSNS = {
    "deal": 25_117,
    "reveal": 32_792,
    "showdown": 237_018,
    "muck": 15_000,
}

# Wall-clock proof generation seconds per circuit (README benchmark table).
CIRCUIT_PROOF_SECONDS = {
    "deal": 0.055,
    "reveal": 0.050,
    "showdown": 0.165,
    "muck": 0.045,
}

# Default number of each circuit a single Texas Hold'em hand commits.
# deal once, reveal flop/turn/river (3), showdown once.
DEFAULT_CIRCUIT_MIX = {"deal": 1, "reveal": 3, "showdown": 1, "muck": 0}


@dataclass
class CostInputs:
    hands_per_day: int = 10_000
    players_per_hand: int = 6
    circuit_mix: dict[str, int] = field(
        default_factory=lambda: dict(DEFAULT_CIRCUIT_MIX)
    )
    proof_bytes: int = 14_624
    base_fee_stroops: int = 100
    # Soroban CPU instruction resource price. The network charges per
    # instruction; Stellar's reference compute price is on the order of
    # tens of stroops per 1 000 instructions. Keep it tunable.
    stroops_per_1k_insns: float = 25.0
    # Disk read/write resource prices (stroops per byte). Soroban's defaults
    # are much smaller than the CPU term but non-zero for proof blobs.
    stroops_per_write_byte: float = 0.01
    stroops_per_read_byte: float = 0.005
    # Extra (non-proof) transaction per player per hand (fold/call/bet/etc.).
    player_action_txs_per_hand: int = 1
    # Off-chain RPC / indexer calls per hand, and their XLM cost.
    rpc_calls_per_hand: int = 12
    rpc_cost_xlm_per_call: float = 0.0
    # MPC committee compute.
    node_count: int = 3
    node_hourly_cost_xlm: float = 0.0
    # Optional USD conversion.
    xlm_price_usd: float = 0.0


def circuit_count(mix: dict[str, int]) -> int:
    return sum(mix.values())


def onchain_stroops_per_hand(inp: CostInputs) -> dict[str, float]:
    """CPU, byte and base-fee components of the on-chain cost, in stroops."""
    mix = inp.circuit_mix
    cpu_insns = sum(CIRCUIT_CPU_INSNS.get(c, 0) * n for c, n in mix.items())
    proof_txs = circuit_count(mix)
    action_txs = inp.players_per_hand * inp.player_action_txs_per_hand
    tx_count = proof_txs + action_txs

    cpu_stroops = cpu_insns * (inp.stroops_per_1k_insns / 1000.0)
    # Proof bytes are written once per proof tx, read once at verification.
    write_bytes = inp.proof_bytes * proof_txs
    read_bytes = inp.proof_bytes * proof_txs
    byte_stroops = (
        write_bytes * inp.stroops_per_write_byte
        + read_bytes * inp.stroops_per_read_byte
    )
    base_stroops = tx_count * inp.base_fee_stroops

    return {
        "cpu_stroops": cpu_stroops,
        "byte_stroops": byte_stroops,
        "base_fee_stroops": base_stroops,
        "cpu_insns": float(cpu_insns),
        "tx_count": float(tx_count),
    }


def mpc_stroops_per_hand(inp: CostInputs) -> dict[str, float]:
    """Committee compute cost per hand, in stroops, from node-seconds."""
    proof_seconds = sum(
        CIRCUIT_PROOF_SECONDS.get(c, 0) * n for c, n in inp.circuit_mix.items()
    )
    # A proof is generated collaboratively: each of the active nodes spends
    # roughly the full proof wall-clock (co-noir is not embarrassingly
    # parallel across parties), so node-seconds = seconds * node_count.
    node_seconds = proof_seconds * inp.node_count
    node_stroops = (
        node_seconds / 3600.0 * inp.node_hourly_cost_xlm * STROOPS_PER_XLM
    )
    return {
        "proof_seconds_per_hand": proof_seconds,
        "node_seconds_per_hand": node_seconds,
        "node_stroops": node_stroops,
    }


def estimate(inp: CostInputs) -> dict:
    onchain = onchain_stroops_per_hand(inp)
    mpc = mpc_stroops_per_hand(inp)

    rpc_stroops = (
        inp.rpc_calls_per_hand * inp.rpc_cost_xlm_per_call * STROOPS_PER_XLM
    )

    onchain_stroops = (
        onchain["cpu_stroops"] + onchain["byte_stroops"] + onchain["base_fee_stroops"]
    )
    total_stroops = onchain_stroops + mpc["node_stroops"] + rpc_stroops

    per_hand_xlm = total_stroops / STROOPS_PER_XLM
    per_day_xlm = per_hand_xlm * inp.hands_per_day
    per_day_usd = per_day_xlm * inp.xlm_price_usd

    result = {
        "inputs": asdict(inp),
        "per_hand": {
            "cpu_insns": onchain["cpu_insns"],
            "tx_count": onchain["tx_count"],
            "onchain_stroops": onchain_stroops,
            "onchain_xlm": onchain_stroops / STROOPS_PER_XLM,
            "cpu_xlm": onchain["cpu_stroops"] / STROOPS_PER_XLM,
            "byte_xlm": onchain["byte_stroops"] / STROOPS_PER_XLM,
            "base_fee_xlm": onchain["base_fee_stroops"] / STROOPS_PER_XLM,
            "rpc_xlm": rpc_stroops / STROOPS_PER_XLM,
            "mpc_node_seconds": mpc["node_seconds_per_hand"],
            "mpc_xlm": mpc["node_stroops"] / STROOPS_PER_XLM,
            "total_xlm": per_hand_xlm,
        },
        "per_day": {
            "hands": inp.hands_per_day,
            "onchain_xlm": onchain_stroops / STROOPS_PER_XLM * inp.hands_per_day,
            "mpc_xlm": mpc["node_stroops"] / STROOPS_PER_XLM * inp.hands_per_day,
            "rpc_xlm": rpc_stroops / STROOPS_PER_XLM * inp.hands_per_day,
            "total_xlm": per_day_xlm,
            "total_usd": per_day_usd,
            "proofs_per_day": circuit_count(inp.circuit_mix) * inp.hands_per_day,
        },
    }
    return result


def render_markdown(result: dict) -> str:
    ph = result["per_hand"]
    pd = result["per_day"]
    inp = result["inputs"]
    lines = [
        "# StellPoker operator cost model",
        "",
        f"- Hands/day: **{pd['hands']:,}**",
        f"- Players/hand: **{inp['players_per_hand']}**",
        f"- Circuits/hand: **{', '.join(f'{k}={v}' for k, v in inp['circuit_mix'].items() if v)}**",
        f"- Proof size: **{inp['proof_bytes']:,} bytes**",
        f"- MPC nodes: **{inp['node_count']}**",
        "",
        "## Per hand",
        "",
        "| Component | XLM | Notes |",
        "|---|---:|---|",
        f"| On-chain CPU | {ph['cpu_xlm']:.8f} | {ph['cpu_insns']:,.0f} instructions |",
        f"| On-chain bytes | {ph['byte_xlm']:.8f} | proof read+write |",
        f"| Base fees | {ph['base_fee_xlm']:.8f} | {ph['tx_count']:.0f} transactions |",
        f"| RPC/indexer | {ph['rpc_xlm']:.8f} | off-chain |",
        f"| MPC compute | {ph['mpc_xlm']:.8f} | {ph['mpc_node_seconds']:.3f} node-seconds |",
        f"| **Total** | **{ph['total_xlm']:.8f}** | |",
        "",
        "## Per day",
        "",
        "| Component | XLM |",
        "|---|---:|",
        f"| On-chain | {pd['onchain_xlm']:,.4f} |",
        f"| MPC compute | {pd['mpc_xlm']:,.4f} |",
        f"| RPC/indexer | {pd['rpc_xlm']:,.4f} |",
        f"| **Total** | **{pd['total_xlm']:,.4f}** |",
    ]
    if inp["xlm_price_usd"] > 0:
        lines.append(f"| **Total (USD @ {inp['xlm_price_usd']})** | **${pd['total_usd']:,.2f}** |")
    lines.append("")
    lines.append(f"- Proofs generated per day: **{pd['proofs_per_day']:,}**")
    return "\n".join(lines)


def parse_mix(raw: str) -> dict[str, int]:
    mix = dict(DEFAULT_CIRCUIT_MIX)
    if not raw:
        return mix
    for part in raw.split(","):
        if not part.strip():
            continue
        key, _, value = part.partition("=")
        key = key.strip().lower()
        if key not in CIRCUIT_CPU_INSNS:
            raise argparse.ArgumentTypeError(
                f"unknown circuit '{key}' (known: {', '.join(CIRCUIT_CPU_INSNS)})"
            )
        mix[key] = int(value)
    return mix


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="Estimate XLM fees + MPC compute cost per StellPoker hand.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument("--hands-per-day", type=int, default=10_000)
    p.add_argument("--players-per-hand", type=int, default=6)
    p.add_argument(
        "--circuit-mix",
        type=parse_mix,
        default=dict(DEFAULT_CIRCUIT_MIX),
        help="comma list, e.g. deal=1,reveal=3,showdown=1",
    )
    p.add_argument("--proof-bytes", type=int, default=14_624)
    p.add_argument("--base-fee-stroops", type=int, default=100)
    p.add_argument("--stroops-per-1k-insns", type=float, default=25.0)
    p.add_argument("--stroops-per-write-byte", type=float, default=0.01)
    p.add_argument("--stroops-per-read-byte", type=float, default=0.005)
    p.add_argument("--player-action-txs-per-hand", type=int, default=1)
    p.add_argument("--rpc-calls-per-hand", type=int, default=12)
    p.add_argument("--rpc-cost-xlm", type=float, default=0.0, dest="rpc_cost_xlm_per_call")
    p.add_argument("--node-count", type=int, default=3)
    p.add_argument("--node-hourly-cost-xlm", type=float, default=0.0)
    p.add_argument(
        "--xlm-price-usd",
        type=float,
        default=0.0,
        help="if > 0, also report totals in USD",
    )
    p.add_argument("--json", action="store_true", help="emit JSON instead of markdown")
    p.add_argument(
        "--scenario-priority",
        action="store_true",
        help="preset for low-volume, high-value priority tables",
    )
    return p


def inputs_from_args(args: argparse.Namespace) -> CostInputs:
    inp = CostInputs(
        hands_per_day=args.hands_per_day,
        players_per_hand=args.players_per_hand,
        circuit_mix=args.circuit_mix,
        proof_bytes=args.proof_bytes,
        base_fee_stroops=args.base_fee_stroops,
        stroops_per_1k_insns=args.stroops_per_1k_insns,
        stroops_per_write_byte=args.stroops_per_write_byte,
        stroops_per_read_byte=args.stroops_per_read_byte,
        player_action_txs_per_hand=args.player_action_txs_per_hand,
        rpc_calls_per_hand=args.rpc_calls_per_hand,
        rpc_cost_xlm_per_call=args.rpc_cost_xlm_per_call,
        node_count=args.node_count,
        node_hourly_cost_xlm=args.node_hourly_cost_xlm,
        xlm_price_usd=args.xlm_price_usd,
    )
    if args.scenario_priority:
        # Priority tables: fewer hands, more overhead tolerated. Nothing is
        # changed that the user did not also pass explicitly.
        inp.players_per_hand = 6
    return inp


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.hands_per_day < 0 or args.node_count < 1:
        print("hands-per-day must be >= 0 and node-count >= 1", file=sys.stderr)
        return 2

    result = estimate(inputs_from_args(args))
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(render_markdown(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
