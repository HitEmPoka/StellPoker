# Operator Cost Model

Issue #586. Committee fee distribution
([Committee Fee Distribution](committee-fee-distribution.md)) pays nodes in
proportion to stake, but an operator deciding whether to run a table needs to
know what a hand *costs*. `scripts/cost_model.py` estimates it.

```bash
python3 scripts/cost_model.py --hands-per-day 10000
```

The tool is a calculator over explicitly-named parameters, not a source of
truth: network resource prices change, and proof timings depend on hardware.
Override the defaults with values measured on your own deployment
(`scripts/collect_percentile_benchmarks.py`, `scripts/bench_circuit_gas.py`).

## Inputs

| Flag | Default | Meaning |
|---|---|---|
| `--hands-per-day` | `10000` | Volume. |
| `--players-per-hand` | `6` | Drives player-action transactions. |
| `--circuit-mix` | `deal=1,reveal=3,showdown=1` | Circuits committed per hand. |
| `--proof-bytes` | `14624` | UltraHonk proof size (bytes). |
| `--base-fee-stroops` | `100` | Per-transaction base fee. |
| `--stroops-per-1k-insns` | `25` | Soroban CPU instruction resource price. |
| `--stroops-per-write-byte` / `--stroops-per-read-byte` | `0.01` / `0.005` | Ledger I/O resource prices. |
| `--player-action-txs-per-hand` | `1` | Non-proof txs per player per hand. |
| `--rpc-calls-per-hand` | `12` | Off-chain RPC/indexer calls. |
| `--rpc-cost-xlm` | `0` | XLM per RPC call (self-hosted often ~0). |
| `--node-count` | `3` | Active MPC parties. |
| `--node-hourly-cost-xlm` | `0` | Amortized node cost in XLM/hour. |
| `--xlm-price-usd` | `0` | If set, also report USD. |
| `--json` | off | Emit JSON instead of markdown. |

## Model

### On-chain fees

Per hand, the coordinator submits one proof transaction per circuit in the mix
plus `players × player_action_txs_per_hand` action transactions. For each:

```
base_fee       = base_fee_stroops × tx_count
cpu_fee        = Σ_circuit(cpu_insns × count) × stroops_per_1k_insns / 1000
byte_fee       = proof_bytes × proof_txs
                   × (stroops_per_write_byte + stroops_per_read_byte)
```

Per-circuit opcode counts come from the project's own benchmarks
(deal 25 117, reveal 32 792, showdown 237 018, muck 15 000; see the README
"Circuit Benchmarking" table).

### MPC compute

Proof generation is collaborative: each of the `node_count` active parties
spends roughly the full proof wall-clock (co-noir is not embarrassingly
parallel across parties). So

```
node_seconds_per_hand = Σ_circuit(proof_seconds × count) × node_count
mpc_xlm_per_hand      = node_seconds_per_hand / 3600 × node_hourly_cost_xlm
```

Proof-second defaults also come from the README benchmark table.

### Totals

```
per_hand_xlm = (onchain + mpc + rpc_calls × rpc_cost_xlm) in XLM
per_day_xlm  = per_hand_xlm × hands_per_day
                       1 XLM = 10 000 000 stroops
```

## Example

```bash
python3 scripts/cost_model.py \
  --hands-per-day 50000 \
  --node-hourly-cost-xlm 2.0 \
  --rpc-cost-xlm 0.00001 \
  --xlm-price-usd 0.40
```

prints a per-hand and per-day breakdown, e.g.:

```
| Component       | XLM        | Notes                    |
|-----------------|-----------:|--------------------------|
| On-chain CPU    | 0.00…      | 446 056 instructions     |
| On-chain bytes  | 0.00…      | proof read+write         |
| Base fees       | 0.00…      | 15 transactions          |
| RPC/indexer     | 0.00…      | off-chain                |
| MPC compute     | 0.00…      | 1.560 node-seconds       |
| **Total**       | **0.00…**  |                          |
```

Add `--json` to feed the numbers into a spreadsheet or a fee-setting decision.

## Using it with fee distribution

The per-hand cost is the floor an operator should recover from rake before a
table is profitable. Compare `per_day.total_xlm` against the rake the table
produces, then check the node's share under
[Committee Fee Distribution](committee-fee-distribution.md). Operators running
priority tables can weigh the cost against the priority class assigned by the
[MPC session preemption policy](mpc-preemption-policy.md).
