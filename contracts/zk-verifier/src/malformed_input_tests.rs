//! Malformed input test matrix for ZK-verifier (Issue #507).
//!
//! This module extends the edge-case testing from Issue #141 by systematically
//! testing every class of malformed public input that the verifier might receive.
//! Every test asserts that malformed inputs return a typed VerifierError, never
//! panic or trap.
//!
//! Test Matrix Coverage:
//! - Wrong field counts (public_inputs byte length)
//! - Wrong commitment counts (hand_commitments vector length)
//! - Wrong board indices count
//! - Public input mismatches (deck_root, cards, indices)
//! - Wrong proof size
//! - Invalid VK data (already covered in main tests)
//!
//! All tests follow the Soroban SDK test pattern: mock auth, typed error
//! assertions, no unwrap/expect on error paths.

use crate::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Bytes, BytesN, Env,
};

fn setup() -> (Env, ZkVerifierContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ZkVerifierContract, ());
    let client = ZkVerifierContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn dummy_vk(env: &Env) -> Bytes {
    // Minimal valid VK structure (ultrahonk-soroban-verifier expects 1824 bytes)
    let mut arr = [0u8; 1824];
    Bytes::from_slice(env, &arr)
}

fn dummy_proof(env: &Env) -> Bytes {
    // Valid-length proof (14624 bytes as defined by PROOF_BYTES constant)
    let arr = [0u8; 14624];
    Bytes::from_slice(env, &arr)
}

fn dummy_bytes32(env: &Env, tag: u8) -> BytesN<32> {
    let mut arr = [tag; 32];
    BytesN::from_array(env, &arr)
}

// ========================================================================
// Deal Circuit Tests (20 fields = 640 bytes)
// ========================================================================

#[test]
fn test_verify_deal_wrong_field_count_too_few() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Deal expects 640 bytes (20 fields), provide only 19 fields = 608 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 608]);
    let deck_root = dummy_bytes32(&env, 1);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 2)];

    let result = client.try_verify_deal(&proof, &public_inputs, &deck_root, &hand_commitments);
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_deal_wrong_field_count_too_many() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Deal expects 640 bytes, provide 21 fields = 672 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 672]);
    let deck_root = dummy_bytes32(&env, 1);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 2)];

    let result = client.try_verify_deal(&proof, &public_inputs, &deck_root, &hand_commitments);
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_deal_too_many_commitments() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 640]);
    let deck_root = dummy_bytes32(&env, 1);
    // MAX_PLAYERS is 6, provide 7 commitments
    let hand_commitments = vec![
        &env,
        dummy_bytes32(&env, 2),
        dummy_bytes32(&env, 3),
        dummy_bytes32(&env, 4),
        dummy_bytes32(&env, 5),
        dummy_bytes32(&env, 6),
        dummy_bytes32(&env, 7),
        dummy_bytes32(&env, 8),
    ];

    let result = client.try_verify_deal(&proof, &public_inputs, &deck_root, &hand_commitments);
    assert_eq!(result, Err(Ok(VerifierError::WrongCommitmentCount)));
}

#[test]
fn test_verify_deal_deck_root_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 640];
    // Set deck_root at field index 1 (bytes 32..64) to a specific value
    for i in 32..64 {
        public_inputs_vec[i] = 0xAA;
    }
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    // Provide a different deck_root
    let deck_root = dummy_bytes32(&env, 0xBB);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 2)];

    let result = client.try_verify_deal(&proof, &public_inputs, &deck_root, &hand_commitments);
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_deal_hand_commitment_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 640];
    // Set hand_commitment[0] at field index 2 (bytes 64..96)
    for i in 64..96 {
        public_inputs_vec[i] = 0xCC;
    }
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let deck_root = dummy_bytes32(&env, 0);
    // Provide a mismatched commitment
    let hand_commitments = vec![&env, dummy_bytes32(&env, 0xDD)];

    let result = client.try_verify_deal(&proof, &public_inputs, &deck_root, &hand_commitments);
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

// ========================================================================
// Reveal Circuit Tests (25 fields = 800 bytes)
// ========================================================================

#[test]
fn test_verify_reveal_wrong_field_count_too_few() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Reveal expects 800 bytes (25 fields), provide 24 fields = 768 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 768]);
    let deck_root = dummy_bytes32(&env, 1);
    let revealed_cards = vec![&env, 0u32, 1u32, 2u32];
    let revealed_indices = vec![&env, 0u32, 1u32, 2u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_reveal_wrong_field_count_too_many() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Reveal expects 800 bytes, provide 26 fields = 832 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 832]);
    let deck_root = dummy_bytes32(&env, 1);
    let revealed_cards = vec![&env, 0u32, 1u32, 2u32];
    let revealed_indices = vec![&env, 0u32, 1u32, 2u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_reveal_deck_root_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 800];
    // Set deck_root at field index 0 (bytes 0..32)
    for i in 0..32 {
        public_inputs_vec[i] = 0xEE;
    }
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let deck_root = dummy_bytes32(&env, 0xFF);
    let revealed_cards = vec![&env, 0u32];
    let revealed_indices = vec![&env, 0u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_reveal_cards_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 800];
    // revealed_cards[0] at field index 19 (bytes 608..640), set to card value 10
    public_inputs_vec[639] = 10;
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let deck_root = dummy_bytes32(&env, 0);
    // Provide mismatched card value
    let revealed_cards = vec![&env, 99u32];
    let revealed_indices = vec![&env, 0u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_reveal_indices_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 800];
    // revealed_indices[0] at field index 22 (bytes 704..736), set to index 5
    public_inputs_vec[735] = 5;
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let deck_root = dummy_bytes32(&env, 0);
    let revealed_cards = vec![&env, 0u32];
    // Provide mismatched index value
    let revealed_indices = vec![&env, 99u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_reveal_mismatched_vector_lengths() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 800]);
    let deck_root = dummy_bytes32(&env, 0);
    // Mismatched lengths: 2 cards but 3 indices
    let revealed_cards = vec![&env, 0u32, 1u32];
    let revealed_indices = vec![&env, 0u32, 1u32, 2u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_reveal_too_many_cards() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::RevealBoardValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 800]);
    let deck_root = dummy_bytes32(&env, 0);
    // Reveal board can reveal max 3 cards at a time (flop), provide 4
    let revealed_cards = vec![&env, 0u32, 1u32, 2u32, 3u32];
    let revealed_indices = vec![&env, 0u32, 1u32, 2u32, 3u32];

    let result = client.try_verify_reveal(
        &proof,
        &public_inputs,
        &deck_root,
        &revealed_cards,
        &revealed_indices,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

// ========================================================================
// Showdown Circuit Tests (27 fields = 864 bytes)
// ========================================================================

#[test]
fn test_verify_showdown_wrong_field_count_too_few() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Showdown expects 864 bytes (27 fields), provide 26 fields = 832 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 832]);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 1)];
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 2);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_showdown_wrong_field_count_too_many() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    // Showdown expects 864 bytes, provide 28 fields = 896 bytes
    let public_inputs = Bytes::from_slice(&env, &[0u8; 896]);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 1)];
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 2);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputSizeError)));
}

#[test]
fn test_verify_showdown_too_many_commitments() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 864]);
    // MAX_PLAYERS is 6, provide 7 commitments
    let hand_commitments = vec![
        &env,
        dummy_bytes32(&env, 1),
        dummy_bytes32(&env, 2),
        dummy_bytes32(&env, 3),
        dummy_bytes32(&env, 4),
        dummy_bytes32(&env, 5),
        dummy_bytes32(&env, 6),
        dummy_bytes32(&env, 7),
    ];
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 8);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::WrongCommitmentCount)));
}

#[test]
fn test_verify_showdown_wrong_board_indices_count() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 864]);
    let hand_commitments = vec![&env, dummy_bytes32(&env, 1)];
    // Board indices count must be exactly 5 (flop + turn + river), provide 4
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32];
    let deck_root = dummy_bytes32(&env, 2);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::WrongBoardIndicesCount)));
}

#[test]
fn test_verify_showdown_deck_root_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 864];
    // Set deck_root at field index 12 (bytes 384..416)
    for i in 384..416 {
        public_inputs_vec[i] = 0x11;
    }
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let hand_commitments = vec![&env, dummy_bytes32(&env, 0)];
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 0x22);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_showdown_hand_commitment_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 864];
    // hand_commitments[0] at field index 1 (bytes 32..64)
    for i in 32..64 {
        public_inputs_vec[i] = 0x33;
    }
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let hand_commitments = vec![&env, dummy_bytes32(&env, 0x44)];
    let board_indices = vec![&env, 0u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 0);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

#[test]
fn test_verify_showdown_board_indices_mismatch() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::ShowdownValid, &dummy_vk(&env), &1);

    let proof = dummy_proof(&env);
    let mut public_inputs_vec = vec![0u8; 864];
    // board_indices[0] at field index 7 (bytes 224..256), set to index 10
    public_inputs_vec[255] = 10;
    let public_inputs = Bytes::from_slice(&env, &public_inputs_vec);
    
    let hand_commitments = vec![&env, dummy_bytes32(&env, 0)];
    // Provide mismatched first board index
    let board_indices = vec![&env, 99u32, 1u32, 2u32, 3u32, 4u32];
    let deck_root = dummy_bytes32(&env, 0);

    let result = client.try_verify_showdown(
        &proof,
        &public_inputs,
        &hand_commitments,
        &board_indices,
        &deck_root,
    );
    assert_eq!(result, Err(Ok(VerifierError::PublicInputMismatch)));
}

// ========================================================================
// Generic Proof Size Tests
// ========================================================================

#[test]
fn test_verify_proof_wrong_size_too_small() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    // Proof must be exactly 14624 bytes, provide 14623
    let proof = Bytes::from_slice(&env, &[0u8; 14623]);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 640]);

    let result = client.try_verify_proof(&CircuitType::DealValid, &proof, &public_inputs);
    assert_eq!(result, Err(Ok(VerifierError::ProofSizeError)));
}

#[test]
fn test_verify_proof_wrong_size_too_large() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    // Proof must be exactly 14624 bytes, provide 14625
    let proof = Bytes::from_slice(&env, &[0u8; 14625]);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 640]);

    let result = client.try_verify_proof(&CircuitType::DealValid, &proof, &public_inputs);
    assert_eq!(result, Err(Ok(VerifierError::ProofSizeError)));
}

#[test]
fn test_verify_proof_empty() {
    let (env, client, admin) = setup();
    client.set_verification_key(&admin, &CircuitType::DealValid, &dummy_vk(&env), &1);

    let proof = Bytes::new(&env);
    let public_inputs = Bytes::from_slice(&env, &[0u8; 640]);

    let result = client.try_verify_proof(&CircuitType::DealValid, &proof, &public_inputs);
    assert_eq!(result, Err(Ok(VerifierError::ProofSizeError)));
}
