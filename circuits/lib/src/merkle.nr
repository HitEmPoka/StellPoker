// Merkle tree utilities using Poseidon2 hashing.
//
// Used for the deck commitment: a Merkle root over 52 card commitments.
// Tree depth for 52 leaves = ceil(log2(52)) = 6 (64-leaf tree, 12 padding zeros).

pub global TREE_DEPTH: u32 = 6;
pub global TREE_LEAVES: u32 = 64;

// Hash two children to produce a parent node.
pub fn hash_pair(left: Field, right: Field) -> Field {
    std::hash::poseidon2_permutation([left, right, 0, 0], 4)[0]
}

// Verify a Merkle proof for a leaf at a given index.
pub fn verify_merkle_proof(
    leaf: Field,
    index: u32,
    proof: [Field; 6],
    root: Field,
) {
    let mut current = leaf;
    let mut idx = index;

    for i in 0..6 {
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

// Compute Merkle root from 64 leaves.
// Leaves 52-63 should be zero (padding).
pub fn compute_merkle_root(leaves: [Field; 64]) -> Field {
    let h0_0 = hash_pair(0, 0);

    // 64 -> 32
    let mut layer: [Field; 32] = [0; 32];
    for i in 0..26 {
        layer[i] = hash_pair(leaves[2 * i], leaves[2 * i + 1]);
    }
    for i in 26..32 {
        layer[i] = h0_0;
    }

    let h1_0 = hash_pair(h0_0, h0_0);

    // 32 -> 16
    let mut layer1: [Field; 16] = [0; 16];
    for i in 0..13 {
        layer1[i] = hash_pair(layer[2 * i], layer[2 * i + 1]);
    }
    for i in 13..16 {
        layer1[i] = h1_0;
    }

    let h2_0 = hash_pair(h1_0, h1_0);

    // 16 -> 8
    let mut layer2: [Field; 8] = [0; 8];
    for i in 0..6 {
        layer2[i] = hash_pair(layer1[2 * i], layer1[2 * i + 1]);
    }
    layer2[6] = hash_pair(layer1[12], layer1[13]);
    layer2[7] = h2_0;

    // 8 -> 4
    let mut layer3: [Field; 4] = [0; 4];
    for i in 0..3 {
        layer3[i] = hash_pair(layer2[2 * i], layer2[2 * i + 1]);
    }
    layer3[3] = hash_pair(layer2[6], layer2[7]);

    // 4 -> 2
    let h0 = hash_pair(layer3[0], layer3[1]);
    let h1 = hash_pair(layer3[2], layer3[3]);

    // root
    hash_pair(h0, h1)
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
