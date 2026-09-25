//! Shared Noir/Rust test vectors (#527).
//!
//! The circuits under `circuits/` and the Rust services agree on a handful of
//! hash constructions: the Poseidon2 pair and triple hashes, the card, hand
//! and board commitments built from them, Merkle roots and inclusion proofs
//! over card commitments, and the batch packing digests of #529. Each side
//! used to pin its own expected values by hand, which is how the two drift.
//!
//! This crate is the single source of truth. [`generate`] computes every
//! vector from fixed inputs with the same `taceo_poseidon2` permutation the
//! coordinator uses, and the binary renders the result twice from that one
//! value: `vectors.json` for Rust consumers and `circuits/lib/src/test_vectors.nr`,
//! a Noir test module that asserts the library reproduces each vector. Both
//! files are committed; `cargo run -p stellpoker-test-vectors -- check` fails
//! when either differs from a fresh render, and CI runs it, so a change to a
//! hash construction on one side cannot land without regenerating the vectors
//! that the other side is tested against.

use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "stellpoker.test-vectors/v1";
pub const JSON_PATH: &str = "circuits/test-vectors/vectors.json";
pub const NOIR_PATH: &str = "circuits/lib/src/test_vectors.nr";

/// Domain tags from `circuits/lib/src/packing.nr`.
pub const DOMAIN_INPUTS: u64 = 0x5350494e;
pub const DOMAIN_HAND: u64 = 0x53504844;
pub const DOMAIN_BATCH: u64 = 0x53504254;

// ── Hash constructions mirrored from circuits/lib ─────────────────────────

/// `poseidon2_permutation([a, b, 0, 0], 4)[0]` (`commitments::poseidon2_hash_2`).
pub fn hash_2(a: Fr, b: Fr) -> Fr {
    taceo_poseidon2::bn254::t4::permutation(&[a, b, Fr::from(0u64), Fr::from(0u64)])[0]
}

/// `poseidon2_permutation([a, b, c, 0], 4)[0]` (`commitments::poseidon2_hash_3`).
pub fn hash_3(a: Fr, b: Fr, c: Fr) -> Fr {
    taceo_poseidon2::bn254::t4::permutation(&[a, b, c, Fr::from(0u64)])[0]
}

pub fn commit_card(card: u64, salt: Fr) -> Fr {
    hash_2(Fr::from(card), salt)
}

pub fn commit_hand(c1: Fr, c2: Fr) -> Fr {
    hash_2(c1, c2)
}

pub fn commit_range_hand(card1: u64, card2: u64, blinding: Fr) -> Fr {
    hash_3(Fr::from(card1), Fr::from(card2), blinding)
}

pub fn commit_hand_omaha(c1: Fr, c2: Fr, c3: Fr, c4: Fr) -> Fr {
    commit_hand(commit_hand(c1, c2), commit_hand(c3, c4))
}

pub fn commit_board_3(c1: u64, c2: u64, c3: u64, salt: Fr) -> Fr {
    hash_3(hash_2(Fr::from(c1), Fr::from(c2)), Fr::from(c3), salt)
}

pub fn commit_board_1(card: u64, prev: Fr, salt: Fr) -> Fr {
    hash_3(prev, Fr::from(card), salt)
}

/// Root of a power-of-two leaf array, bottom-up as `compute_merkle_root_generic`.
pub fn merkle_root(leaves: &[Fr]) -> Fr {
    assert!(
        leaves.len().is_power_of_two(),
        "leaf count must be a power of two"
    );
    let mut layer = leaves.to_vec();
    while layer.len() > 1 {
        layer = layer
            .chunks(2)
            .map(|pair| hash_2(pair[0], pair[1]))
            .collect();
    }
    layer[0]
}

/// Sibling path for `index`, level 0 first, as `compute_merkle_proof_generic`.
pub fn merkle_proof(leaves: &[Fr], index: usize) -> Vec<Fr> {
    assert!(index < leaves.len(), "index out of range");
    let mut layer = leaves.to_vec();
    let mut idx = index;
    let mut path = Vec::new();
    while layer.len() > 1 {
        path.push(layer[idx ^ 1]);
        layer = layer
            .chunks(2)
            .map(|pair| hash_2(pair[0], pair[1]))
            .collect();
        idx >>= 1;
    }
    path
}

/// Fold a leaf up its path, as `verify_merkle_proof_generic` does.
pub fn merkle_root_from_proof(leaf: Fr, index: usize, path: &[Fr]) -> Fr {
    let mut current = leaf;
    let mut idx = index;
    for sibling in path {
        current = if idx & 1 == 0 {
            hash_2(current, *sibling)
        } else {
            hash_2(*sibling, current)
        };
        idx >>= 1;
    }
    current
}

pub fn pack_fields(domain: u64, fields: &[Fr]) -> Fr {
    let mut acc = hash_2(Fr::from(domain), Fr::from(fields.len() as u64));
    for f in fields {
        acc = hash_2(acc, *f);
    }
    acc
}

pub fn pack_hand(hand_id: Fr, circuit_id: Fr, inputs: &[Fr]) -> Fr {
    let header = hash_3(Fr::from(DOMAIN_HAND), hand_id, circuit_id);
    hash_2(header, pack_fields(DOMAIN_INPUTS, inputs))
}

pub fn pack_batch(digests: &[Fr], num_hands: usize) -> Fr {
    assert!(
        num_hands <= digests.len(),
        "num_hands exceeds batch capacity"
    );
    let mut acc = hash_2(Fr::from(DOMAIN_BATCH), Fr::from(num_hands as u64));
    for d in &digests[..num_hands] {
        acc = hash_2(acc, *d);
    }
    acc
}

// ── Encoding ───────────────────────────────────────────────────────────────

/// `0x` + 64 lowercase hex digits, big-endian. Parses in Noir as a `Field`
/// literal and in Rust through [`parse_field`].
pub fn field_hex(value: Fr) -> String {
    format!("0x{}", hex::encode(value.into_bigint().to_bytes_be()))
}

pub fn parse_field(text: &str) -> Fr {
    let digits = text
        .strip_prefix("0x")
        .expect("field literal must start with 0x");
    let bytes = hex::decode(digits).expect("field literal must be hex");
    Fr::from_be_bytes_mod_order(&bytes)
}

// ── Vector shapes ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash2Vector {
    pub a: String,
    pub b: String,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash3Vector {
    pub a: String,
    pub b: String,
    pub c: String,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardCommitment {
    pub card: u64,
    pub salt: String,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandCommitment {
    pub card1: u64,
    pub salt1: String,
    pub card2: u64,
    pub salt2: String,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeHandCommitment {
    pub card1: u64,
    pub card2: u64,
    pub blinding: String,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmahaCommitment {
    pub cards: [u64; 4],
    pub salts: [String; 4],
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardCommitment {
    pub flop: [u64; 3],
    pub flop_salt: String,
    pub flop_out: String,
    pub turn: u64,
    pub turn_salt: String,
    pub turn_out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleProofVector {
    pub index: usize,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleTreeVector {
    pub depth: usize,
    /// Leaves 0..cards are `commit_card(i, salt_i)`; the rest are zero padding.
    pub cards: usize,
    pub salt_offset: u64,
    pub leaves: Vec<String>,
    pub root: String,
    pub proofs: Vec<MerkleProofVector>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackFieldsVector {
    pub domain: u64,
    pub fields: Vec<String>,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackHandVector {
    pub hand_id: u64,
    pub circuit_id: u64,
    pub inputs: Vec<String>,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackBatchVector {
    pub digests: Vec<String>,
    pub num_hands: usize,
    pub out: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vectors {
    pub schema: String,
    pub generator: String,
    pub poseidon2_hash_2: Vec<Hash2Vector>,
    pub poseidon2_hash_3: Vec<Hash3Vector>,
    pub commit_card: Vec<CardCommitment>,
    pub commit_hand: Vec<HandCommitment>,
    pub commit_range_hand: Vec<RangeHandCommitment>,
    pub commit_hand_omaha: Vec<OmahaCommitment>,
    pub commit_board: Vec<BoardCommitment>,
    pub merkle: Vec<MerkleTreeVector>,
    pub pack_fields: Vec<PackFieldsVector>,
    pub pack_hand: Vec<PackHandVector>,
    pub pack_batch: Vec<PackBatchVector>,
}

// ── Generation ─────────────────────────────────────────────────────────────

fn salt(i: u64, offset: u64) -> Fr {
    Fr::from(offset + i)
}

fn merkle_vector(
    depth: usize,
    cards: usize,
    salt_offset: u64,
    proof_indices: &[usize],
) -> MerkleTreeVector {
    let size = 1usize << depth;
    let mut leaves = vec![Fr::from(0u64); size];
    for (i, leaf) in leaves.iter_mut().enumerate().take(cards) {
        *leaf = commit_card(i as u64, salt(i as u64, salt_offset));
    }
    MerkleTreeVector {
        depth,
        cards,
        salt_offset,
        leaves: leaves.iter().map(|f| field_hex(*f)).collect(),
        root: field_hex(merkle_root(&leaves)),
        proofs: proof_indices
            .iter()
            .map(|&index| MerkleProofVector {
                index,
                path: merkle_proof(&leaves, index)
                    .iter()
                    .map(|f| field_hex(*f))
                    .collect(),
            })
            .collect(),
    }
}

/// Every vector, from fixed inputs. Deterministic: the same output on every run.
pub fn generate() -> Vectors {
    let big = parse_field("0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593efffffff");
    let pairs = [(0u64, 0u64), (1, 2), (42, 123_456_789), (51, 1)];
    let poseidon2_hash_2 = pairs
        .iter()
        .map(|&(a, b)| Hash2Vector {
            a: field_hex(Fr::from(a)),
            b: field_hex(Fr::from(b)),
            out: field_hex(hash_2(Fr::from(a), Fr::from(b))),
        })
        .chain(std::iter::once(Hash2Vector {
            a: field_hex(big),
            b: field_hex(big),
            out: field_hex(hash_2(big, big)),
        }))
        .collect();
    let triples = [(0u64, 0u64, 0u64), (1, 2, 3), (7, 51, 999)];
    let poseidon2_hash_3 = triples
        .iter()
        .map(|&(a, b, c)| Hash3Vector {
            a: field_hex(Fr::from(a)),
            b: field_hex(Fr::from(b)),
            c: field_hex(Fr::from(c)),
            out: field_hex(hash_3(Fr::from(a), Fr::from(b), Fr::from(c))),
        })
        .collect();
    let commit_card_vectors = [(0u64, 1u64), (42, 123_456_789), (51, 987_654_321)]
        .iter()
        .map(|&(card, s)| CardCommitment {
            card,
            salt: field_hex(Fr::from(s)),
            out: field_hex(commit_card(card, Fr::from(s))),
        })
        .collect();
    let commit_hand_vectors = [((10u64, 111u64), (25u64, 222u64)), ((7, 123), (51, 456))]
        .iter()
        .map(|&((c1, s1), (c2, s2))| HandCommitment {
            card1: c1,
            salt1: field_hex(Fr::from(s1)),
            card2: c2,
            salt2: field_hex(Fr::from(s2)),
            out: field_hex(commit_hand(
                commit_card(c1, Fr::from(s1)),
                commit_card(c2, Fr::from(s2)),
            )),
        })
        .collect();
    let commit_range_hand_vectors = vec![RangeHandCommitment {
        card1: 12,
        card2: 25,
        blinding: field_hex(Fr::from(4242u64)),
        out: field_hex(commit_range_hand(12, 25, Fr::from(4242u64))),
    }];
    let omaha_cards = [10u64, 25, 39, 44];
    let omaha_salts = [111u64, 222, 333, 444];
    let omaha_commits: Vec<Fr> = omaha_cards
        .iter()
        .zip(omaha_salts.iter())
        .map(|(&c, &s)| commit_card(c, Fr::from(s)))
        .collect();
    let commit_hand_omaha_vectors = vec![OmahaCommitment {
        cards: omaha_cards,
        salts: omaha_salts.map(|s| field_hex(Fr::from(s))),
        out: field_hex(commit_hand_omaha(
            omaha_commits[0],
            omaha_commits[1],
            omaha_commits[2],
            omaha_commits[3],
        )),
    }];
    let flop_out = commit_board_3(3, 17, 40, Fr::from(5555u64));
    let commit_board_vectors = vec![BoardCommitment {
        flop: [3, 17, 40],
        flop_salt: field_hex(Fr::from(5555u64)),
        flop_out: field_hex(flop_out),
        turn: 48,
        turn_salt: field_hex(Fr::from(6666u64)),
        turn_out: field_hex(commit_board_1(48, flop_out, Fr::from(6666u64))),
    }];
    let merkle = vec![
        merkle_vector(6, 52, 1000, &[0, 17, 51]),
        merkle_vector(7, 104, 2000, &[0, 103]),
        merkle_vector(9, 416, 3000, &[415]),
    ];
    let fields: Vec<Fr> = (1..=4u64).map(Fr::from).collect();
    let pack_fields_vectors = vec![
        PackFieldsVector {
            domain: DOMAIN_INPUTS,
            fields: vec![],
            out: field_hex(pack_fields(DOMAIN_INPUTS, &[])),
        },
        PackFieldsVector {
            domain: DOMAIN_INPUTS,
            fields: fields.iter().map(|f| field_hex(*f)).collect(),
            out: field_hex(pack_fields(DOMAIN_INPUTS, &fields)),
        },
    ];
    let hand_inputs: Vec<Vec<Fr>> = (0..3u64)
        .map(|h| (0..16u64).map(|j| Fr::from(h * 16 + j + 1)).collect())
        .collect();
    let hand_digests: Vec<Fr> = hand_inputs
        .iter()
        .enumerate()
        .map(|(h, inputs)| pack_hand(Fr::from(100 + h as u64), Fr::from(3u64), inputs))
        .collect();
    let pack_hand_vectors = hand_inputs
        .iter()
        .enumerate()
        .map(|(h, inputs)| PackHandVector {
            hand_id: 100 + h as u64,
            circuit_id: 3,
            inputs: inputs.iter().map(|f| field_hex(*f)).collect(),
            out: field_hex(hand_digests[h]),
        })
        .collect();
    let mut padded = hand_digests.clone();
    padded.extend([Fr::from(0u64); 5]);
    let pack_batch_vectors = vec![
        PackBatchVector {
            digests: padded.iter().map(|f| field_hex(*f)).collect(),
            num_hands: 3,
            out: field_hex(pack_batch(&padded, 3)),
        },
        PackBatchVector {
            digests: padded.iter().map(|f| field_hex(*f)).collect(),
            num_hands: 1,
            out: field_hex(pack_batch(&padded, 1)),
        },
    ];
    Vectors {
        schema: SCHEMA.to_string(),
        generator: "circuits/test-vectors (cargo run -p stellpoker-test-vectors -- generate)"
            .to_string(),
        poseidon2_hash_2,
        poseidon2_hash_3,
        commit_card: commit_card_vectors,
        commit_hand: commit_hand_vectors,
        commit_range_hand: commit_range_hand_vectors,
        commit_hand_omaha: commit_hand_omaha_vectors,
        commit_board: commit_board_vectors,
        merkle,
        pack_fields: pack_fields_vectors,
        pack_hand: pack_hand_vectors,
        pack_batch: pack_batch_vectors,
    }
}

// ── Rendering ──────────────────────────────────────────────────────────────

pub fn render_json(vectors: &Vectors) -> String {
    let mut text = serde_json::to_string_pretty(vectors).expect("vectors serialise");
    text.push('\n');
    text
}

fn noir_array(values: &[String]) -> String {
    format!("[{}]", values.join(", "))
}

/// The Noir test module. Every test calls the library function the vector
/// describes and asserts the committed value, so `nargo test` in
/// `circuits/lib` is the Noir side of the drift check.
pub fn render_noir(v: &Vectors) -> String {
    let mut out = String::new();
    out.push_str("// Shared test vectors (Issue #527).\n");
    out.push_str("//\n// GENERATED by circuits/test-vectors; do not edit by hand.\n");
    out.push_str("// Regenerate with: cargo run -p stellpoker-test-vectors -- generate\n");
    out.push_str("// The same values live in circuits/test-vectors/vectors.json for Rust\n");
    out.push_str("// consumers; CI fails when either file is stale.\n\n");
    out.push_str("use crate::commitments;\nuse crate::merkle;\nuse crate::packing;\n\n");

    out.push_str("#[test]\nfn test_vectors_poseidon2_hash_2() {\n");
    for h in &v.poseidon2_hash_2 {
        out.push_str(&format!(
            "    assert(commitments::poseidon2_hash_2({}, {}) == {});\n",
            h.a, h.b, h.out
        ));
    }
    out.push_str("}\n\n#[test]\nfn test_vectors_poseidon2_hash_3() {\n");
    for h in &v.poseidon2_hash_3 {
        out.push_str(&format!(
            "    assert(commitments::poseidon2_hash_3({}, {}, {}) == {});\n",
            h.a, h.b, h.c, h.out
        ));
    }
    out.push_str("}\n\n#[test]\nfn test_vectors_commitments() {\n");
    for c in &v.commit_card {
        out.push_str(&format!(
            "    assert(commitments::commit_card({}, {}) == {});\n",
            c.card, c.salt, c.out
        ));
    }
    for h in &v.commit_hand {
        out.push_str(&format!(
            "    assert(commitments::commit_hand(commitments::commit_card({}, {}), commitments::commit_card({}, {})) == {});\n",
            h.card1, h.salt1, h.card2, h.salt2, h.out
        ));
    }
    for r in &v.commit_range_hand {
        out.push_str(&format!(
            "    assert(commitments::commit_range_hand({}, {}, {}) == {});\n",
            r.card1, r.card2, r.blinding, r.out
        ));
    }
    for o in &v.commit_hand_omaha {
        out.push_str(&format!(
            "    assert(commitments::commit_hand_omaha(commitments::commit_card({}, {}), commitments::commit_card({}, {}), commitments::commit_card({}, {}), commitments::commit_card({}, {})) == {});\n",
            o.cards[0], o.salts[0], o.cards[1], o.salts[1], o.cards[2], o.salts[2], o.cards[3], o.salts[3], o.out
        ));
    }
    for b in &v.commit_board {
        out.push_str(&format!(
            "    let flop = commitments::commit_board_3({}, {}, {}, {});\n    assert(flop == {});\n    assert(commitments::commit_board_1({}, flop, {}) == {});\n",
            b.flop[0], b.flop[1], b.flop[2], b.flop_salt, b.flop_out, b.turn, b.turn_salt, b.turn_out
        ));
    }
    out.push_str("}\n");

    for t in &v.merkle {
        let leaves = 1usize << t.depth;
        out.push_str(&format!(
            "\n#[test]\nfn test_vectors_merkle_depth_{}() {{\n",
            t.depth
        ));
        out.push_str(&format!(
            "    let leaves: [Field; {}] = {};\n",
            leaves,
            noir_array(&t.leaves)
        ));
        out.push_str(&format!("    let root = merkle::compute_merkle_root_generic::<{}, {}>(leaves);\n    assert(root == {});\n", leaves, t.depth, t.root));
        for p in &t.proofs {
            out.push_str(&format!(
                "    let path_{idx}: [Field; {d}] = {path};\n    assert(merkle::compute_merkle_proof_generic::<{n}, {d}>(leaves, {idx}) == path_{idx});\n    merkle::verify_merkle_proof_generic::<{d}>(leaves[{idx}], {idx}, path_{idx}, root);\n",
                idx = p.index, d = t.depth, n = leaves, path = noir_array(&p.path)
            ));
        }
        out.push_str("}\n");
    }

    out.push_str("\n#[test]\nfn test_vectors_packing() {\n");
    for p in &v.pack_fields {
        let domain = match p.domain {
            DOMAIN_INPUTS => "packing::DOMAIN_INPUTS",
            DOMAIN_HAND => "packing::DOMAIN_HAND",
            _ => "packing::DOMAIN_BATCH",
        };
        if p.fields.is_empty() {
            out.push_str(&format!("    let empty: [Field; 0] = [];\n    assert(packing::pack_fields({}, empty) == {});\n", domain, p.out));
        } else {
            out.push_str(&format!(
                "    assert(packing::pack_fields({}, {}) == {});\n",
                domain,
                noir_array(&p.fields),
                p.out
            ));
        }
    }
    for (i, h) in v.pack_hand.iter().enumerate() {
        out.push_str(&format!(
            "    let hand_{i} = packing::pack_hand({}, {}, {});\n    assert(hand_{i} == {});\n",
            h.hand_id,
            h.circuit_id,
            noir_array(&h.inputs),
            h.out
        ));
    }
    for b in &v.pack_batch {
        out.push_str(&format!(
            "    assert(packing::pack_batch({}, {}) == {});\n",
            noir_array(&b.digests),
            b.num_hands,
            b.out
        ));
    }
    out.push_str("}\n");
    out
}

// ── Paths and drift check ──────────────────────────────────────────────────

/// Repository root, two levels above this crate.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

pub struct Rendered {
    pub relative_path: &'static str,
    pub content: String,
}

pub fn render_all(vectors: &Vectors) -> Vec<Rendered> {
    vec![
        Rendered {
            relative_path: JSON_PATH,
            content: render_json(vectors),
        },
        Rendered {
            relative_path: NOIR_PATH,
            content: render_noir(vectors),
        },
    ]
}

/// Paths (relative to the repo root) whose committed content differs from a
/// fresh render, with a reason. Empty when everything is current.
pub fn stale_files(root: &Path) -> Vec<(String, &'static str)> {
    render_all(&generate())
        .into_iter()
        .filter_map(
            |r| match std::fs::read_to_string(root.join(r.relative_path)) {
                Ok(current) if current == r.content => None,
                Ok(_) => Some((r.relative_path.to_string(), "differs from a fresh render")),
                Err(_) => Some((r.relative_path.to_string(), "missing")),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET_PATTERN_FREE_VECTOR: &str =
        "0x01bd538c2ee014ed5141b29e9ae240bf8db3fe5b9a38629a9647cf8d76c01737";

    #[test]
    fn permutation_matches_barretenberg_vector() {
        let out = taceo_poseidon2::bn254::t4::permutation(&[
            Fr::from(0u64),
            Fr::from(1u64),
            Fr::from(2u64),
            Fr::from(3u64),
        ]);
        assert_eq!(field_hex(out[0]), SECRET_PATTERN_FREE_VECTOR);
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(render_json(&generate()), render_json(&generate()));
        assert_eq!(render_noir(&generate()), render_noir(&generate()));
    }

    #[test]
    fn field_hex_round_trips() {
        let value = hash_2(Fr::from(5u64), Fr::from(6u64));
        assert_eq!(parse_field(&field_hex(value)), value);
        assert_eq!(field_hex(value).len(), 66);
    }

    #[test]
    fn merkle_proofs_verify_against_the_root() {
        for tree in generate().merkle {
            let leaves: Vec<Fr> = tree.leaves.iter().map(|s| parse_field(s)).collect();
            let root = parse_field(&tree.root);
            assert_eq!(merkle_root(&leaves), root);
            for proof in tree.proofs {
                let path: Vec<Fr> = proof.path.iter().map(|s| parse_field(s)).collect();
                assert_eq!(path.len(), tree.depth);
                assert_eq!(
                    merkle_root_from_proof(leaves[proof.index], proof.index, &path),
                    root
                );
                assert_ne!(
                    merkle_root_from_proof(leaves[proof.index], proof.index ^ 1, &path),
                    root
                );
            }
        }
    }

    #[test]
    fn packing_is_length_prefixed_and_order_bound() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        assert_ne!(
            pack_fields(DOMAIN_INPUTS, &[a, b]),
            pack_fields(DOMAIN_INPUTS, &[a, b, Fr::from(0u64)])
        );
        assert_ne!(pack_batch(&[a, b], 2), pack_batch(&[b, a], 2));
        assert_eq!(
            pack_batch(&[a, b, Fr::from(9u64)], 2),
            pack_batch(&[a, b, Fr::from(7u64)], 2)
        );
    }

    #[test]
    fn committed_vectors_are_current() {
        let stale = stale_files(&repo_root());
        assert!(
            stale.is_empty(),
            "stale vector files: {stale:?}; run: cargo run -p stellpoker-test-vectors -- generate"
        );
    }
}
