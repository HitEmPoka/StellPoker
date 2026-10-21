// Merkle tree utilities using Poseidon2 hashing.
//
// Supports variable tree depths to accommodate decks of different sizes.
// Default: depth 6 for 52-card deck in 64-leaf tree.
//
// Tree depth calculation: ceil(log2(deck_size + padding))
// - 52 cards: depth = 6 (64-leaf tree)
// - 104 cards: depth = 7 (128-leaf tree)
// - 200 cards: depth = 8 (256-leaf tree)

// Default tree configuration for standard 52-card deck
pub global TREE_DEPTH: u32 = 6;
pub global TREE_LEAVES: u32 = 64;

// Deck size configuration (can be overridden per circuit)
pub global DECK_SIZE: u32 = 52;

// Largest depth the generic helpers accept (Issue #528). Every depth in use
// sits well below it: 6 for a 52-card deck, 9 for an eight-deck shoe, 12 for
// the rank table. Each extra level costs one Poseidon2 permutation per proof
// verification and doubles the leaf array a root computation has to hold, so
// the cap turns a typo in a `<let DEPTH>` argument into a compile error
// ("merkle depth exceeds MAX_TREE_DEPTH") instead of a circuit with millions
// of gates. The per-depth constraint table lives in docs/merkle-depth.md.
pub global MAX_TREE_DEPTH: u32 = 16;

// Number of leaves a tree of `DEPTH` levels holds (2^DEPTH). The loop bound is
// a compile-time constant, so this folds to a constant and costs no gates.
pub fn leaves_for_depth<let DEPTH: u32>() -> u32 {
    let mut leaves: u32 = 1;
    for _ in 0..DEPTH {
        leaves *= 2;
    }
    leaves
}

// Hash two children to produce a parent node.
pub fn hash_pair(left: Field, right: Field) -> Field {
    std::hash::poseidon2_permutation([left, right, 0, 0], 4)[0]
}

// Verify a Merkle proof for a leaf at a given index (generic depth version).
pub fn verify_merkle_proof_generic<let DEPTH: u32>(
    leaf: Field,
    index: u32,
    proof: [Field; DEPTH],
    root: Field,
) {
    std::static_assert(DEPTH <= MAX_TREE_DEPTH, "merkle depth exceeds MAX_TREE_DEPTH");
    // Bits of `index` above DEPTH would otherwise be ignored, so index 64 in a
    // depth-6 tree would verify with leaf 0's path. Reject it instead.
    assert(index < leaves_for_depth::<DEPTH>(), "merkle index out of range for depth");
    let mut current = leaf;
    let mut idx = index;

    for i in 0..DEPTH {
        let sibling = proof[i];
        if (idx & 1) == 0 {
            current = hash_pair(current, sibling);
        } else {
            current = hash_pair(sibling, current);
        }
        idx = idx >> 1;
    }

    assert(current == root, "merkle proof invalid");
}

// Verify a Merkle proof for a leaf at a given index (fixed 6-level tree, backward compatible).
pub fn verify_merkle_proof(
    leaf: Field,
    index: u32,
    proof: [Field; 6],
    root: Field,
) {
    verify_merkle_proof_generic(leaf, index, proof, root);
}

// Compute Merkle root from leaves with variable tree depth.
// `DEPTH` is passed as a compile-time constant (log2 of the leaf count) so the
// bottom-up loop is unrollable in a constrained context.
//   - 64 leaves  (52 cards + 12 padding)            -> DEPTH = 6
//   - 128 leaves (104 cards + 24 padding)           -> DEPTH = 7
//   - 512 leaves (multi-deck shoe, 6 or 8 decks)    -> DEPTH = 9
// Leaves beyond the card count should be zero (padding).
pub fn compute_merkle_root_generic<let LEAVES: u32, let DEPTH: u32>(
    leaves: [Field; LEAVES],
) -> Field {
    std::static_assert(DEPTH <= MAX_TREE_DEPTH, "merkle depth exceeds MAX_TREE_DEPTH");
    assert(LEAVES == leaves_for_depth::<DEPTH>(), "LEAVES must equal 2^DEPTH");
    // Bottom-up tree construction.
    let mut capacity: u32 = LEAVES;
    let mut current_layer = leaves;
    for _layer_level in 0..DEPTH {
        let mut next_layer_size = capacity / 2;
        let mut next_layer: [Field; LEAVES] = [0; LEAVES];

        // Compute parent hashes for this layer
        for i in 0..next_layer_size {
            let left = if 2 * i < LEAVES { current_layer[2 * i] } else { 0 };
            let right = if 2 * i + 1 < LEAVES { current_layer[2 * i + 1] } else { 0 };
            next_layer[i] = hash_pair(left, right);
        }

        // Prepare for next layer - shift down and update capacity
        current_layer = next_layer;
        capacity = next_layer_size;
    }

    current_layer[0]
}

// Compute Merkle root from 64 leaves (backward compatible).
// Leaves 52-63 should be zero (padding).
pub fn compute_merkle_root(leaves: [Field; 64]) -> Field {
    compute_merkle_root_generic::<64, 6>(leaves)
}

// Compute the Merkle sibling path for a single leaf so an inclusion proof can
// be produced for tests and table lookups. `path[i]` is the sibling of the
// leaf's ancestor at level i (level 0 immediately above the leaf).
pub fn compute_merkle_proof_generic<let LEAVES: u32, let DEPTH: u32>(
    leaves: [Field; LEAVES],
    index: u32,
) -> [Field; DEPTH] {
    std::static_assert(DEPTH <= MAX_TREE_DEPTH, "merkle depth exceeds MAX_TREE_DEPTH");
    assert(LEAVES == leaves_for_depth::<DEPTH>(), "LEAVES must equal 2^DEPTH");
    assert(index < LEAVES, "merkle index out of range for depth");
    let mut current_layer = leaves;
    let mut path: [Field; DEPTH] = [0; DEPTH];
    let mut idx = index;
    for level in 0..DEPTH {
        let sibling = current_layer[idx ^ 1];
        path[level] = sibling;
        let next_size = LEAVES >> (level + 1);
        let mut next_layer: [Field; LEAVES] = [0; LEAVES];
        for i in 0..next_size {
            next_layer[i] = hash_pair(current_layer[2 * i], current_layer[2 * i + 1]);
        }
        current_layer = next_layer;
        idx = idx >> 1;
    }
    path
}

#[test]
fn test_merkle_root_deterministic() {
    let mut leaves: [Field; 64] = [0; 64];
    for i in 0..52 {
        leaves[i] = i as Field;
    }

    let root = compute_merkle_root(leaves);
    let root2 = compute_merkle_root(leaves);
    assert(root == root2);
}

#[test]
fn test_merkle_root_generic_64() {
    let mut leaves: [Field; 64] = [0; 64];
    for i in 0..52 {
        leaves[i] = i as Field;
    }

    let root = compute_merkle_root_generic::<64, 6>(leaves);
    let root_standard = compute_merkle_root(leaves);
    assert(root == root_standard, "generic and standard should match for 64 leaves");
}

#[test]
fn test_merkle_root_generic_128() {
    let mut leaves: [Field; 128] = [0; 128];
    for i in 0..104 {
        leaves[i] = i as Field;
    }

    let root1 = compute_merkle_root_generic::<128, 7>(leaves);
    let root2 = compute_merkle_root_generic::<128, 7>(leaves);
    assert(root1 == root2, "generic merkle should be deterministic for 128 leaves");
}

// -- Depth boundary tests (Issue #528) -----------------------------------------
//
// `roundtrip` fills every leaf, computes the root, then proves and verifies
// the first, middle and last leaf, so both the root builder and the proof
// builder are exercised at each depth. It is used for depths 1..10 (up to
// 1024 leaves). Above that the root computation is too large to run in a unit
// test, so depths 11..MAX_TREE_DEPTH verify a synthetic path against the root
// folded from it, which is what the verifier does in a circuit anyway.

fn roundtrip<let LEAVES: u32, let DEPTH: u32>() {
    let mut leaves: [Field; LEAVES] = [0; LEAVES];
    for i in 0..LEAVES {
        leaves[i] = (i as Field) + 1;
    }
    let root = compute_merkle_root_generic::<LEAVES, DEPTH>(leaves);
    let indices = [0, LEAVES / 2, LEAVES - 1];
    for k in 0..3 {
        let idx = indices[k];
        let path = compute_merkle_proof_generic::<LEAVES, DEPTH>(leaves, idx);
        verify_merkle_proof_generic::<DEPTH>(leaves[idx], idx, path, root);
    }
}

// Fold a leaf up a synthetic path, the same walk `verify_merkle_proof_generic`
// performs, returning the root it lands on.
fn root_from_path<let DEPTH: u32>(leaf: Field, index: u32, path: [Field; DEPTH]) -> Field {
    let mut current = leaf;
    let mut idx = index;
    for i in 0..DEPTH {
        if (idx & 1) == 0 {
            current = hash_pair(current, path[i]);
        } else {
            current = hash_pair(path[i], current);
        }
        idx = idx >> 1;
    }
    current
}

fn verify_synthetic<let DEPTH: u32>() {
    let mut path: [Field; DEPTH] = [0; DEPTH];
    for i in 0..DEPTH {
        path[i] = (i as Field) + 1000;
    }
    let last = leaves_for_depth::<DEPTH>() - 1;
    let leaf: Field = 42;
    verify_merkle_proof_generic::<DEPTH>(leaf, 0, path, root_from_path::<DEPTH>(leaf, 0, path));
    verify_merkle_proof_generic::<DEPTH>(leaf, last, path, root_from_path::<DEPTH>(leaf, last, path));
}

#[test]
fn test_leaves_for_depth() {
    assert(leaves_for_depth::<1>() == 2);
    assert(leaves_for_depth::<6>() == 64);
    assert(leaves_for_depth::<9>() == 512);
    assert(leaves_for_depth::<12>() == 4096);
    assert(leaves_for_depth::<16>() == 65536);
}

#[test]
fn test_depth_1_roundtrip() {
    roundtrip::<2, 1>();
}

#[test]
fn test_depth_2_roundtrip() {
    roundtrip::<4, 2>();
}

#[test]
fn test_depth_3_roundtrip() {
    roundtrip::<8, 3>();
}

#[test]
fn test_depth_4_roundtrip() {
    roundtrip::<16, 4>();
}

#[test]
fn test_depth_5_roundtrip() {
    roundtrip::<32, 5>();
}

#[test]
fn test_depth_6_roundtrip() {
    roundtrip::<64, 6>();
}

#[test]
fn test_depth_7_roundtrip() {
    roundtrip::<128, 7>();
}

#[test]
fn test_depth_8_roundtrip() {
    roundtrip::<256, 8>();
}

#[test]
fn test_depth_9_roundtrip() {
    roundtrip::<512, 9>();
}

#[test]
fn test_depth_10_roundtrip() {
    roundtrip::<1024, 10>();
}

#[test]
fn test_depth_11_verify() {
    verify_synthetic::<11>();
}

#[test]
fn test_depth_12_verify() {
    verify_synthetic::<12>();
}

#[test]
fn test_depth_13_verify() {
    verify_synthetic::<13>();
}

#[test]
fn test_depth_14_verify() {
    verify_synthetic::<14>();
}

#[test]
fn test_depth_15_verify() {
    verify_synthetic::<15>();
}

#[test]
fn test_max_depth_verify() {
    verify_synthetic::<16>();
}

// Index 64 in a depth-6 tree carries leaf 0's path bits. Before the range
// check it verified against leaf 0's root; now it is rejected outright.
#[test(should_fail_with = "merkle index out of range for depth")]
fn test_index_beyond_capacity_rejected() {
    let mut leaves: [Field; 64] = [0; 64];
    for i in 0..52 {
        leaves[i] = (i as Field) + 1;
    }
    let root = compute_merkle_root(leaves);
    let path = compute_merkle_proof_generic::<64, 6>(leaves, 0);
    verify_merkle_proof_generic::<6>(leaves[0], 64, path, root);
}

#[test(should_fail_with = "merkle index out of range for depth")]
fn test_proof_builder_rejects_index_beyond_capacity() {
    let leaves: [Field; 2] = [1, 2];
    let _ = compute_merkle_proof_generic::<2, 1>(leaves, 2);
}

#[test(should_fail_with = "merkle proof invalid")]
fn test_depth_1_wrong_root_rejected() {
    let leaves: [Field; 2] = [1, 2];
    let root = compute_merkle_root_generic::<2, 1>(leaves);
    let path = compute_merkle_proof_generic::<2, 1>(leaves, 1);
    verify_merkle_proof_generic::<1>(leaves[1], 1, path, root + 1);
}

#[test(should_fail_with = "merkle proof invalid")]
fn test_max_depth_tampered_sibling_rejected() {
    let mut path: [Field; 16] = [0; 16];
    for i in 0..16 {
        path[i] = (i as Field) + 1000;
    }
    let leaf: Field = 42;
    let root = root_from_path::<16>(leaf, 12345, path);
    path[15] = path[15] + 1;
    verify_merkle_proof_generic::<16>(leaf, 12345, path, root);
}

#[test(should_fail_with = "LEAVES must equal 2^DEPTH")]
fn test_leaf_count_must_match_depth() {
    let leaves: [Field; 64] = [0; 64];
    let _ = compute_merkle_root_generic::<64, 5>(leaves);
}
