//! Benchmark: deal-phase MPC message (de)serialization latency vs message
//! size (Issue #580, expanding on #100's round-trip latency measurements).
//!
//! `services/node` has no `[lib]` target, so this bench — like the
//! `committee-registry` crate's existing `bench_main.rs` — is self-contained
//! rather than importing crate internals.
//!
//! What this measures: the deal phase's `public_inputs.json` payload
//! (`run_proof_generation` in `session.rs` reads and parses this file after
//! every proof generation) grows with the table's player count, since the
//! `deal_valid` circuit's public outputs are `[Field; MAX_PLAYERS]`-shaped
//! arrays (see `circuits/deal_valid/src/main.nr`). This bench models that
//! scaling directly: serializing/deserializing a public-inputs-shaped JSON
//! payload at message sizes corresponding to 2, 3, 4, 5, and 6 players,
//! giving a message-size axis alongside #100's existing player-count axis.
//!
//! This is a microbenchmark of the (de)serialization step specifically, not
//! a full live multi-node MPC round trip — that would need a running
//! cluster and isn't CI-friendly. Results and methodology are documented in
//! `services/node/docs/MPC_LATENCY_BENCHMARKS.md`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PublicInputsPayload {
    /// One Field-sized hex string per player slot, mirroring
    /// `deal_valid`'s `[Field; MAX_PLAYERS]` public outputs.
    hole_card_commitments: Vec<String>,
    deck_root: String,
    num_players: u32,
}

/// Build a payload sized for `n_players`, with each Field represented the
/// same way `run_proof_generation` reads it back: a hex-encoded string.
fn payload_for(n_players: usize) -> PublicInputsPayload {
    let field_hex = "a".repeat(64); // 32-byte Field, hex-encoded
    PublicInputsPayload {
        hole_card_commitments: vec![field_hex.clone(); n_players],
        deck_root: field_hex,
        num_players: n_players as u32,
    }
}

fn bench_message_size_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpc_deal_phase_message_size");

    for n_players in [2usize, 3, 4, 5, 6] {
        let payload = payload_for(n_players);
        let serialized =
            serde_json::to_vec(&payload).expect("payload must serialize for benchmark setup");

        group.bench_with_input(
            BenchmarkId::new("serialize", n_players),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let bytes = serde_json::to_vec(payload).unwrap();
                    criterion::black_box(bytes);
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("deserialize", n_players),
            &serialized,
            |b, bytes| {
                b.iter(|| {
                    let payload: PublicInputsPayload = serde_json::from_slice(bytes).unwrap();
                    criterion::black_box(payload);
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_message_size_matrix);
criterion_main!(benches);
