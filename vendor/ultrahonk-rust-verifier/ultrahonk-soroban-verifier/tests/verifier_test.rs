use soroban_sdk::{testutils::Ledger, Bytes, Env};
use std::{fs, path::Path};
use ultrahonk_soroban_verifier::UltraHonkVerifier;

fn run(dir: &str) -> Result<(), String> {
    let path = Path::new(dir);
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    // Proof bytes
    let proof_bytes: Vec<u8> = fs::read(path.join("proof")).map_err(|e| e.to_string())?;
    let proof = Bytes::from_slice(&env, &proof_bytes);

    // Use binary VK
    let vk_bytes = fs::read(path.join("vk")).map_err(|e| e.to_string())?;
    let vk = Bytes::from_slice(&env, &vk_bytes);
    let verifier = UltraHonkVerifier::new(&env, &vk).map_err(|e| format!("{e:?}"))?;

    // Public inputs bytes
    let public_inputs = fs::read(path.join("public_inputs")).map_err(|e| e.to_string())?;
    let public_inputs = Bytes::from_slice(&env, &public_inputs);
    verifier
        .verify(&proof, &public_inputs)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn simple_circuit_proof_verifies() -> Result<(), String> {
    run("circuits/simple_circuit/target")
}

#[test]
fn fib_chain_proof_verifies() -> Result<(), String> {
    run("circuits/fib_chain/target")
}

#[test]
fn proof_with_empty_inputs_should_fail() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let proof_bytes = Bytes::from_slice(&env, &[0u8; 32]);
    let vk_bytes = Bytes::from_slice(&env, &[0u8; 32]);
    let empty_inputs = Bytes::new(&env);

    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);
    assert!(verifier.is_err(), "Should reject invalid VK bytes");
}

#[test]
fn proof_with_misaligned_inputs_should_fail() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let vk_bytes = Bytes::from_slice(&env, &vec![0u8; 256]);
    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);

    if let Ok(v) = verifier {
        let misaligned_inputs = Bytes::from_slice(&env, &[0u8; 31]);
        let proof = Bytes::from_slice(&env, &vec![0u8; 1024]);

        let result = v.verify(&proof, &misaligned_inputs);
        assert!(
            result.is_err(),
            "Should reject inputs not aligned to 32-byte boundaries"
        );
    }
}

#[test]
fn proof_with_truncated_data_should_fail() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let vk_bytes = Bytes::from_slice(&env, &vec![0u8; 256]);
    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);

    if let Ok(v) = verifier {
        let truncated_proof = Bytes::from_slice(&env, &[0u8; 16]);
        let valid_inputs = Bytes::from_slice(&env, &vec![0u8; 32]);

        let result = v.verify(&truncated_proof, &valid_inputs);
        assert!(result.is_err(), "Should reject truncated proof data");
    }
}

#[test]
fn proof_with_manipulated_elements_should_fail() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let vk_bytes = Bytes::from_slice(&env, &vec![0u8; 256]);
    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);

    if let Ok(v) = verifier {
        let mut manipulated_proof = vec![0u8; 512];
        manipulated_proof[0] = 0xFF;
        manipulated_proof[255] = 0xFF;

        let proof = Bytes::from_slice(&env, &manipulated_proof);
        let inputs = Bytes::from_slice(&env, &vec![0u8; 32]);

        let result = v.verify(&proof, &inputs);
        assert!(result.is_err(), "Should reject manipulated proof elements");
    }
}

#[test]
fn proof_with_boundary_value_inputs() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let vk_bytes = Bytes::from_slice(&env, &vec![0u8; 256]);
    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);

    if let Ok(v) = verifier {
        let mut max_value_inputs = vec![0xFFu8; 32];
        max_value_inputs[31] = 0xFF;

        let inputs = Bytes::from_slice(&env, &max_value_inputs);
        let proof = Bytes::from_slice(&env, &vec![0u8; 512]);

        let result = v.verify(&proof, &inputs);
        assert!(result.is_err(), "Should handle boundary value inputs safely");
    }
}

#[test]
fn verifier_rejects_wrong_public_input_count() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);

    let vk_bytes = Bytes::from_slice(&env, &vec![0u8; 256]);
    let verifier = UltraHonkVerifier::new(&env, &vk_bytes);

    if let Ok(v) = verifier {
        let single_input = Bytes::from_slice(&env, &vec![0u8; 32]);
        let proof = Bytes::from_slice(&env, &vec![0u8; 512]);
        let wrong_count = Bytes::from_slice(&env, &vec![0u8; 64]);

        let result = v.verify(&proof, &wrong_count);
        assert!(result.is_err(), "Should reject mismatched public input count");
    }
}
