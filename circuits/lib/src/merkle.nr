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
// For 64 leaves (52 cards + 12 padding), depth = 6.
// For 128 leaves (104 cards + 24 padding), depth = 7.
// Leaves beyond card count should be zero (padding).
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
