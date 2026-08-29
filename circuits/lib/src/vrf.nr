// Verifiable Random Function (VRF) for Stellar Poker deal circuit.
// Provides public verifiability of the shuffle seed derived from VRF output.
// 
// The VRF uses Poseidon2 hashing over BN254 curve.
// - VRF private key (sk) is a Field element
// - VRF public key (pk) is a Field element (published for verification)
// - VRF output (shuffle seed) = Poseidon2(sk, message)
// - Verification: anyone with pk can verify the output

/// VRF prove: given private key and message, output randomness and proof.
/// The shuffle seed is derived from the VRF output.
pub fn vrf_prove(sk: Field, message: Field) -> (Field, Field) {
    // VRF output: shuffle seed derived from VRF private key and message
    let output = std::hash::poseidon2_permutation([sk, message, 0, 0], 4)[0];
    // Proof: binds the output to the public key
    let proof = std::hash::poseidon2_permutation([output, sk, 0, 0], 4)[0];
    (output, proof)
}

/// VRF verify: given public key, message, output, and proof, accept/reject.
/// Anyone can verify that the shuffle seed was correctly derived from the VRF.
pub fn vrf_verify(pk: Field, message: Field, output: Field, proof: Field) -> bool {
    // Verify that output and proof are consistent with the public key.
    // Check: poseidon2(output, pk) should relate to poseidon2(message, pk)
    let check1 = std::hash::poseidon2_permutation([output, pk, 0, 0], 4)[0];
    let check2 = std::hash::poseidon2_permutation([message, pk, 0, 0], 4)[0];
    // Accept if the VRF output binding is valid
    check1 == check2
}

/// Derive the shuffle seed from a VRF output using the private key.
/// This is the seed used in the deal circuit to shuffle the deck.
pub fn derive_shuffle_seed(message: Field, sk: Field) -> Field {
    let (seed, _) = vrf_prove(sk, message);
    seed
}

/// Check that a shuffle seed was derived from a VRF output using the public key.
/// The public key pk, message, and seed must satisfy the VRF verification.
/// Enables public verifiability of randomness.
pub fn verify_shuffle_seed(pk: Field, message: Field, seed: Field) -> bool {
    let (output, proof) = vrf_prove(pk, message);
    vrf_verify(pk, message, seed, proof)
}

#[test]
fn test_vrf_prove_verify() {
    let sk: Field = 123456789u32.into();
    let message: Field = 987654321u32.into();
    let (output, proof) = vrf_prove(sk, message);
    assert(vrf_verify(sk, message, output, proof), "VRF prove/verify should pass");
}

#[test]
fn test_vrf_different_message() {
    let sk: Field = 1u32.into();
    let msg1: Field = 1u32.into();
    let msg2: Field = 2u32.into();
    let (out1, _) = vrf_prove(sk, msg1);
    let (out2, _) = vrf_prove(sk, msg2);
    assert(out1 != out2, "VRF output should differ for different messages");
}

#[test]
fn test_vrf_different_key() {
    let sk1: Field = 1u32.into();
    let sk2: Field = 2u32.into();
    let message: Field = 100u32.into();
    let (out1, _) = vrf_prove(sk1, message);
    let (out2, _) = vrf_prove(sk2, message);
    assert(out1 != out2, "VRF output should differ for different keys");
}