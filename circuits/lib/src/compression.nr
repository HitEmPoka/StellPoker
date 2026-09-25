/// Witness compression utilities for permutation arrays.
///
/// Permutation indices for a 52-card deck only require 6 bits each (values 0-51).
/// This module provides bit-packing functions to compress permutation witness data
/// and reduce prover memory footprint, targeting 5-6x compression for 52-element
/// permutation arrays compared to storing them as u32 arrays.

/// Maximum index value that can be represented (supports 6-bit packing for indices 0-51)
pub global MAX_PACKED_INDEX: u32 = 63;
pub global BITS_PER_INDEX: u32 = 6;

/// Pack a 52-element permutation array into a compact representation.
/// Stores 52 indices (6 bits each = 312 bits = 10 u32 values) instead of 52 u32 = 1664 bits.
/// Returns array of packed u32 values (10 elements for 52 indices).
pub fn pack_permutation_52(perm: [u32; 52]) -> [u32; 10] {
    assert_permutation_valid(perm);

    let mut packed: [u32; 10] = [0; 10];

    // Pack 5 indices per u32 (5 * 6 = 30 bits, leaving 2 bits unused)
    // Index positions: [0-4] → packed[0], [5-9] → packed[1], etc.
    for i in 0..52 {
        let packed_idx = i / 5;
        let bit_offset = (i % 5) * 6;
        packed[packed_idx] = packed[packed_idx] | ((perm[i] as u32) << bit_offset);
    }

    packed
}

/// Unpack a compressed permutation back to [u32; 52].
pub fn unpack_permutation_52(packed: [u32; 10]) -> [u32; 52] {
    let mut perm: [u32; 52] = [0; 52];

    for i in 0..52 {
        let packed_idx = i / 5;
        let bit_offset = (i % 5) * 6;
        let mask: u32 = 63; // 6-bit mask
        perm[i] = (packed[packed_idx] >> bit_offset) & mask;
    }

    perm
}

/// Validate that all indices in permutation are in valid range [0, 51]
fn assert_permutation_valid(perm: [u32; 52]) {
    for i in 0..52 {
        assert(perm[i] < 52, "permutation index out of range");
    }
}

#[test]
fn test_permutation_pack_unpack_identity() {
    // Identity permutation [0, 1, 2, ..., 51]
    let mut identity: [u32; 52] = [0; 52];
    for i in 0..52 {
        identity[i] = i as u32;
    }

    let packed = pack_permutation_52(identity);
    let unpacked = unpack_permutation_52(packed);

    for i in 0..52 {
        assert(unpacked[i] == identity[i], "identity permutation mismatch");
    }
}

#[test]
fn test_permutation_pack_unpack_reversed() {
    // Reversed permutation [51, 50, 49, ..., 0]
    let mut reversed: [u32; 52] = [0; 52];
    for i in 0..52 {
        reversed[i] = 51 - i as u32;
    }

    let packed = pack_permutation_52(reversed);
    let unpacked = unpack_permutation_52(packed);

    for i in 0..52 {
        assert(unpacked[i] == reversed[i], "reversed permutation mismatch");
    }
}

#[test]
fn test_permutation_pack_unpack_arbitrary() {
    // Arbitrary permutation (shift by 1)
    let mut shifted: [u32; 52] = [0; 52];
    for i in 0..52 {
        shifted[i] = ((i as u32) + 1) % 52;
    }

    let packed = pack_permutation_52(shifted);
    let unpacked = unpack_permutation_52(packed);

    for i in 0..52 {
        assert(unpacked[i] == shifted[i], "shifted permutation mismatch");
    }
}
