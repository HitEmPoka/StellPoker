use soroban_sdk::{contractclient, Bytes, BytesN, Env, Vec};

#[cfg(test)]
use soroban_sdk::{contract, contractimpl};

/// ZK Verifier contract interface.
/// Matches the interface in contracts/zk-verifier/src/lib.rs
#[cfg(test)]
#[contract]
#[allow(dead_code)]
pub struct ZkVerifierContract;

#[allow(dead_code)]
#[contractclient(name = "ZkVerifierClient")]
pub trait ZkVerifier {
    fn verify_deal(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        hand_commitments: Vec<BytesN<32>>,
    ) -> Result<bool, soroban_sdk::Error>;

    fn verify_reveal(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        revealed_cards: Vec<u32>,
        revealed_indices: Vec<u32>,
    ) -> Result<bool, soroban_sdk::Error>;

    fn verify_showdown(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        hand_commitments: Vec<BytesN<32>>,
        board_indices: Vec<u32>,
        deck_root: BytesN<32>,
    ) -> Result<bool, soroban_sdk::Error>;

    /// VK version recorded by the verifier for a successful proof (Issue #890).
    fn get_proof_vk_version(env: Env, proof_hash: BytesN<32>) -> Option<u32>;
}

/// Mock implementation for tests. In production, the real zk-verifier
/// contract is deployed separately and called cross-contract.
#[cfg(test)]
#[contractimpl]
#[allow(dead_code)]
impl ZkVerifierContract {
    pub fn verify_deal(
        _env: Env,
        _proof: Bytes,
        _public_inputs: Bytes,
        _deck_root: BytesN<32>,
        _hand_commitments: Vec<BytesN<32>>,
    ) -> Result<bool, soroban_sdk::Error> {
        Ok(true)
    }

    pub fn verify_reveal(
        _env: Env,
        _proof: Bytes,
        _public_inputs: Bytes,
        _deck_root: BytesN<32>,
        _revealed_cards: Vec<u32>,
        _revealed_indices: Vec<u32>,
    ) -> Result<bool, soroban_sdk::Error> {
        Ok(true)
    }

    pub fn verify_showdown(
        _env: Env,
        _proof: Bytes,
        _public_inputs: Bytes,
        _hand_commitments: Vec<BytesN<32>>,
        _board_indices: Vec<u32>,
        _deck_root: BytesN<32>,
    ) -> Result<bool, soroban_sdk::Error> {
        Ok(true)
    }

    pub fn get_proof_vk_version(_env: Env, _proof_hash: BytesN<32>) -> Option<u32> {
        Some(1)
    }
}
