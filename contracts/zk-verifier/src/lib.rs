#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env,
    Symbol, Vec,
};
use ultrahonk_soroban_verifier::{UltraHonkVerifier, PROOF_BYTES};

mod governance;

/// Public input layout for each circuit type (field element positions):
///
/// DealValid  (20 fields = 640 bytes):
///   [0]  num_players
///   [1]  deck_root
///   [2..8)  hand_commitments[0..6]
///   [8..14) dealt_card1_indices[0..6]
///   [14..20) dealt_card2_indices[0..6]
///
/// RevealBoardValid  (25 fields = 800 bytes):
///   [0]  deck_root
///   [1]  num_revealed
///   [2]  num_previously_used
///   [3..19) previously_used_indices[0..16]
///   [19..22) revealed_cards[0..3]
///   [22..25) revealed_indices[0..3]
///
/// ShowdownValid  (27 fields = 864 bytes):
///   [0]  num_active_players
///   [1..7)  hand_commitments[0..6]
///   [7..12) board_indices[0..5]
///   [12] deck_root
///   [13..19) hole_card1[0..6]
///   [19..25) hole_card2[0..6]
///   [25] winner_index
///   [26] tie_mask

const SHOWDOWN_FIELD_COUNT: u32 = 27;
const SHOWDOWN_BYTES: u32 = SHOWDOWN_FIELD_COUNT * 32;

const DEAL_FIELD_COUNT: u32 = 20;
const DEAL_BYTES: u32 = DEAL_FIELD_COUNT * 32;

const REVEAL_FIELD_COUNT: u32 = 25;
const REVEAL_BYTES: u32 = REVEAL_FIELD_COUNT * 32;

const MAX_PLAYERS: u32 = 6;
const BOARD_INDICES_COUNT: u32 = 5;

fn ct_bytes32_eq(left: &BytesN<32>, right: &BytesN<32>) -> bool {
    let left_arr = left.to_array();
    let right_arr = right.to_array();
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= left_arr[i] ^ right_arr[i];
    }
    diff == 0
}

fn ct_address_eq(env: &Env, left: &Address, right: &Address) -> bool {
    let left_hash: BytesN<32> = env.crypto().keccak256(&left.to_xdr(env)).into();
    let right_hash: BytesN<32> = env.crypto().keccak256(&right.to_xdr(env)).into();
    ct_bytes32_eq(&left_hash, &right_hash)
}

fn ct_u32_eq(left: u32, right: u32) -> bool {
    (left ^ right) == 0
}

/// ZK Verifier contract for Stellar Poker.
///
/// Uses UltraHonk proof verification via Soroban's native BN254 host functions
/// (Protocol 25 / X-Ray). Each circuit type has its own verification key (VK)
/// stored on-chain. Proofs are verified against their circuit's VK and the
/// provided public inputs.
#[contract]
pub struct ZkVerifierContract;

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerifierError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    NoVkForCircuit = 4,
    VkParseError = 5,
    ProofSizeError = 6,
    VerificationFailed = 7,
    PublicInputSizeError = 8,
    PublicInputMismatch = 9,
    WrongCommitmentCount = 10,
    WrongBoardIndicesCount = 11,
    ContractPaused = 12,
    // Governance (Issue #504): timelock + multi-sig gated upgrades.
    NotAnUpgradeSigner = 13,
    NotEnoughUpgradeApprovals = 14,
    UpgradeTimelockPending = 15,
    NoPendingUpgrade = 16,
    InvalidGovernanceConfig = 17,
    UpgradeAlreadyApproved = 18,
    InvalidVkVersion = 19,
    // Multi-circuit VK hot-swap by ID
    UnknownCircuitId = 20,
    StaleCircuitId = 21,
    UnknownId = 22,
    StaleId = 23,
}

#[contracttype]
#[derive(Clone)]
pub enum CircuitType {
    DealValid,
    RevealBoardValid,
    ShowdownValid,
}

impl CircuitType {
    pub fn default_id(&self) -> u32 {
        match self {
            CircuitType::DealValid => 1,
            CircuitType::RevealBoardValid => 2,
            CircuitType::ShowdownValid => 3,
        }
    }
}

/// Status of a circuit verification key in the registry.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CircuitStatus {
    Active = 1,
    Stale = 2,
    Deprecated = 3,
}

/// Multi-circuit registry entry combining circuit metadata, VK entry, and status.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitEntry {
    pub circuit_id: u32,
    pub vk: VerificationKeyEntry,
    pub status: CircuitStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Admin,
    Vk(CircuitType),
    ProofVerified(BytesN<32>),
    Paused,
    UpgradeSigners,
    UpgradeThreshold,
    UpgradeDelay,
    PendingUpgrade,
    // Multi-circuit VK registry by circuit ID
    VkById(u32),
    CircuitStatus(u32),
}

/// Versioned registry entry for a circuit verification key. The content hash
/// lets deploy tooling and clients pin the exact VK, while `activated_at`
/// provides an unambiguous on-chain activation boundary.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationKeyEntry {
    pub hash: BytesN<32>,
    pub version: u32,
    pub activated_at: u32,
    pub vk_data: Bytes,
}

/// A contract upgrade proposed through the governance path (Issue #504).
/// Approvals accumulate until the configured threshold is reached AND the
/// per-network timelock has elapsed since `started_ledger`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub approvals: Vec<Address>,
    pub started_ledger: u32,
}

#[contractimpl]
impl ZkVerifierContract {
    /// Initialize the verifier with an admin.
    pub fn initialize(env: Env, admin: Address) -> Result<(), VerifierError> {
        if env.storage().instance().has(&StorageKey::Admin) {
            return Err(VerifierError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&StorageKey::Admin, &admin);
        Ok(())
    }

    /// Pause the verifier (admin only). All proof-verification calls revert while paused.
    /// NOTE: for production consider a timelock or multi-sig for unpause.
    pub fn pause(env: Env, admin: Address) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }
        env.storage().instance().set(&StorageKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "verifier_paused"),), admin);
        Ok(())
    }

    /// Unpause the verifier (admin only).
    /// NOTE: for production consider a timelock or multi-sig here.
    pub fn unpause(env: Env, admin: Address) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }
        env.storage().instance().set(&StorageKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "verifier_unpaused"),), admin);
        Ok(())
    }

    /// Returns true if the verifier is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<StorageKey, bool>(&StorageKey::Paused)
            .unwrap_or(false)
    }

    /// Configure N-of-M upgrade governance (admin only).
    ///
    /// `signers` is the full M-signer set, `threshold` the N approvals
    /// required, and `delay_ledgers` the per-network timelock before an
    /// approved upgrade may execute.
    pub fn configure_upgrade_governance(
        env: Env,
        admin: Address,
        signers: Vec<Address>,
        threshold: u32,
        delay_ledgers: u32,
    ) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }
        governance::validate_governance_config(&env, &signers, threshold, delay_ledgers)?;
        governance::store_governance_config(&env, signers, threshold, delay_ledgers);
        env.events()
            .publish((Symbol::new(&env, "governance_configured"),), (threshold, delay_ledgers));
        Ok(())
    }

    /// Propose (and sign) a verifier upgrade to `wasm_hash`.
    ///
    /// Only a configured signer may call this. Returns the number of distinct
    /// approvals collected so far.
    pub fn propose_upgrade(
        env: Env,
        signer: Address,
        wasm_hash: BytesN<32>,
    ) -> Result<u32, VerifierError> {
        signer.require_auth();
        if !governance::governance_configured(&env) {
            return Err(VerifierError::InvalidGovernanceConfig);
        }
        let approvals = governance::propose_upgrade(&env, &signer, wasm_hash.clone())?;
        env.events().publish(
            (Symbol::new(&env, "upgrade_proposed"),),
            (wasm_hash, approvals),
        );
        Ok(approvals)
    }

    /// Execute the pending upgrade once fully approved and its timelock has
    /// elapsed. The target WASM hash is taken from the proposal.
    pub fn execute_upgrade(env: Env) -> Result<(), VerifierError> {
        if !governance::governance_configured(&env) {
            return Err(VerifierError::InvalidGovernanceConfig);
        }
        let pending = governance::load_pending(&env)?;
        let wasm_hash = pending.wasm_hash.clone();
        governance::can_execute(&env, &wasm_hash)?;
        env.deployer().update_current_contract_wasm(wasm_hash.clone());
        governance::clear_pending(&env);
        env.events()
            .publish((Symbol::new(&env, "upgrade_executed"),), wasm_hash);
        Ok(())
    }

    /// Read-only oracle mirroring `execute_upgrade`'s gating: returns `Ok(())`
    /// only when a fully approved proposal whose timelock has elapsed exists.
    /// Lets off-chain watchers (and tests) poll upgrade readiness.
    pub fn can_execute_upgrade(env: Env) -> Result<(), VerifierError> {
        if !governance::governance_configured(&env) {
            return Err(VerifierError::InvalidGovernanceConfig);
        }
        let pending = governance::load_pending(&env)?;
        governance::can_execute(&env, &pending.wasm_hash)?;
        Ok(())
    }

    /// View current upgrade-governance settings.
    pub fn get_upgrade_governance(env: Env) -> (Vec<Address>, u32, u32) {
        (
            governance::load_signers(&env),
            env.storage()
                .instance()
                .get::<StorageKey, u32>(&StorageKey::UpgradeThreshold)
                .unwrap_or(0),
            env.storage()
                .instance()
                .get::<StorageKey, u32>(&StorageKey::UpgradeDelay)
                .unwrap_or(0),
        )
    }

    /// Read the pending upgrade proposal, if any.
    pub fn get_pending_upgrade(env: Env) -> Result<PendingUpgrade, VerifierError> {
        governance::load_pending(&env)
    }

    /// Store a verification key for a circuit type.
    /// Called once per circuit during deployment.
    pub fn set_verification_key(
        env: Env,
        admin: Address,
        circuit: CircuitType,
        vk_data: Bytes,
        version: u32,
    ) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }

        // Validate the VK can be parsed before storing
        UltraHonkVerifier::new(&env, &vk_data).map_err(|_| VerifierError::VkParseError)?;

        if version == 0 {
            return Err(VerifierError::InvalidVkVersion);
        }

        let entry = VerificationKeyEntry {
            hash: env.crypto().keccak256(&vk_data).into(),
            version,
            activated_at: env.ledger().sequence(),
            vk_data,
        };

        if let Some(current) = env
            .storage()
            .persistent()
            .get::<StorageKey, VerificationKeyEntry>(&StorageKey::Vk(circuit.clone()))
        {
            if version <= current.version {
                return Err(VerifierError::InvalidVkVersion);
            }
        }

        env.storage()
            .persistent()
            .set(&StorageKey::Vk(circuit.clone()), &entry);

        env.events().publish(
            (Symbol::new(&env, "vk_set"), circuit),
            (entry.hash.clone(), version, entry.activated_at),
        );
        Ok(())
    }

    /// Return the active VK registry entry for client-side circuit pinning.
    pub fn get_verification_key(
        env: Env,
        circuit: CircuitType,
    ) -> Result<VerificationKeyEntry, VerifierError> {
        env.storage()
            .persistent()
            .get(&StorageKey::Vk(circuit))
            .ok_or(VerifierError::NoVkForCircuit)
    }

    /// Verify an UltraHonk proof for a given circuit type.
    ///
    /// 1. Loads the VK for the circuit type
    /// 2. Validates proof size (14,624 bytes = 457 fields * 32)
    /// 3. Runs full UltraHonk verification (sumcheck + shplonk pairing)
    /// 4. Stores proof hash for auditability
    pub fn verify_proof(
        env: Env,
        circuit: CircuitType,
        proof: Bytes,
        public_inputs: Bytes,
    ) -> Result<bool, VerifierError> {
        // Reject all verification calls while paused
        if env
            .storage()
            .instance()
            .get::<StorageKey, bool>(&StorageKey::Paused)
            .unwrap_or(false)
        {
            return Err(VerifierError::ContractPaused);
        }

        // Check proof size
        if proof.len() as usize != PROOF_BYTES {
            return Err(VerifierError::ProofSizeError);
        }

        // Load VK for this circuit
        let vk_entry: VerificationKeyEntry = env
            .storage()
            .persistent()
            .get(&StorageKey::Vk(circuit))
            .ok_or(VerifierError::NoVkForCircuit)?;

        // Parse VK and create verifier
        let verifier = UltraHonkVerifier::new(&env, &vk_entry.vk_data)
            .map_err(|_| VerifierError::VkParseError)?;

        // Run full UltraHonk verification
        verifier
            .verify(&proof, &public_inputs)
            .map_err(|_| VerifierError::VerificationFailed)?;

        // Store proof hash for auditability
        let proof_hash = env.crypto().keccak256(&proof);
        env.storage()
            .persistent()
            .set(&StorageKey::ProofVerified(proof_hash.clone().into()), &true);

        env.events()
            .publish((Symbol::new(&env, "proof_verified"),), proof_hash);

        Ok(true)
    }

    /// Check if a proof was previously verified.
    pub fn is_proof_verified(env: Env, proof_hash: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::ProofVerified(proof_hash))
            .unwrap_or(false)
    }

    // ====================================================================
    // Helpers — parse and validate public inputs
    // ====================================================================

    /// Check that a 32-byte field element in `public_inputs` at `field_index`
    /// matches an `expected` BytesN<32>.
    fn check_bytes32_field(public_inputs: &Bytes, field_index: u32, expected: &BytesN<32>) -> bool {
        let start = field_index * 32;
        let expected_arr = expected.to_array();
        let mut diff = 0u8;
        for i in 0..32u32 {
            let actual = public_inputs.get(start + i).unwrap_or(0);
            diff |= actual ^ expected_arr[i as usize];
        }
        diff == 0
    }

    /// Extract a u32 from a BN254 field element at `field_index` in public_inputs.
    /// A small integer value is stored as big-endian in the last 4 bytes of the
    /// 32-byte field element.
    fn extract_u32_field(public_inputs: &Bytes, field_index: u32) -> u32 {
        let start = field_index * 32 + 28;
        let b0 = public_inputs.get(start).unwrap_or(0);
        let b1 = public_inputs.get(start + 1).unwrap_or(0);
        let b2 = public_inputs.get(start + 2).unwrap_or(0);
        let b3 = public_inputs.get(start + 3).unwrap_or(0);
        (b0 as u32) << 24 | (b1 as u32) << 16 | (b2 as u32) << 8 | b3 as u32
    }

    /// Check that a u32 value matches the field element at `field_index`.
    fn check_u32_field(public_inputs: &Bytes, field_index: u32, expected: u32) -> bool {
        ct_u32_eq(
            Self::extract_u32_field(public_inputs, field_index),
            expected,
        )
    }

    // ====================================================================
    // Deal proof — validate deck_root and hand_commitments match proof
    // ====================================================================

    /// Verify a deal proof and validate that the proved deck_root and
    /// hand_commitments match the supplied values (which get stored on-chain).
    ///
    /// Public output layout (field indices within public_inputs):
    ///   [0]  num_players
    ///   [1]  deck_root
    ///   [2..8)  hand_commitments[0..6]
    pub fn verify_deal(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        hand_commitments: Vec<BytesN<32>>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != DEAL_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        if hand_commitments.len() > MAX_PLAYERS {
            return Err(VerifierError::WrongCommitmentCount);
        }

        // deck_root at field index 1
        if !Self::check_bytes32_field(&public_inputs, 1, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        // hand_commitments at field indices 2 .. 2 + len
        for i in 0..hand_commitments.len() {
            let expected = hand_commitments
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_bytes32_field(&public_inputs, 2 + i, &expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        Self::verify_proof(env, CircuitType::DealValid, proof, public_inputs)
    }

    // ====================================================================
    // Reveal proof — validate deck_root, revealed cards, and indices
    // ====================================================================

    /// Verify a board reveal proof and validate that the proved deck_root,
    /// revealed card values, and revealed indices match the supplied values.
    ///
    /// Public input/output layout:
    ///   [0]  deck_root  (public input)
    ///   [19..22) revealed_cards[0..3]  (public output)
    ///   [22..25) revealed_indices[0..3]  (public output)
    pub fn verify_reveal(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        revealed_cards: Vec<u32>,
        revealed_indices: Vec<u32>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != REVEAL_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        let num_revealed = revealed_cards.len();
        if num_revealed != revealed_indices.len() || num_revealed > 3 {
            return Err(VerifierError::PublicInputMismatch);
        }

        // deck_root at field index 0 (public input)
        if !Self::check_bytes32_field(&public_inputs, 0, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        // revealed_cards at field indices 19 .. 19 + num_revealed
        for i in 0..num_revealed {
            let expected = revealed_cards
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 19 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        // revealed_indices at field indices 22 .. 22 + num_revealed
        for i in 0..num_revealed {
            let expected = revealed_indices
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 22 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        Self::verify_proof(env, CircuitType::RevealBoardValid, proof, public_inputs)
    }

    // ====================================================================
    // Showdown proof — validate hand_commitments, board_indices, deck_root,
    // and return the proved winner_index / tie_mask outputs
    // ====================================================================

    /// Verify a showdown proof and validate that all game-state parameters
    /// (hand_commitments, board_indices, deck_root) match the on-chain state.
    ///
    /// Public input/output layout:
    ///   [0]  num_active_players
    ///   [1..7)  hand_commitments[0..6]
    ///   [7..12) board_indices[0..5]
    ///   [12] deck_root
    ///   [25] winner_index
    ///   [26] tie_mask
    pub fn verify_showdown(
        env: Env,
        proof: Bytes,
        public_inputs: Bytes,
        hand_commitments: Vec<BytesN<32>>,
        board_indices: Vec<u32>,
        deck_root: BytesN<32>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != SHOWDOWN_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        if hand_commitments.len() > MAX_PLAYERS {
            return Err(VerifierError::WrongCommitmentCount);
        }
        if board_indices.len() != BOARD_INDICES_COUNT {
            return Err(VerifierError::WrongBoardIndicesCount);
        }

        // 1. Verify hand_commitments at field indices 1..7 match stored
        for i in 0..hand_commitments.len() {
            let expected = hand_commitments
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_bytes32_field(&public_inputs, 1 + i, &expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        // 2. Verify board_indices at field indices 7..12 match on-chain dealt indices
        for i in 0..BOARD_INDICES_COUNT {
            let expected = board_indices
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 7 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        // 3. Verify deck_root at field index 12 matches stored
        if !Self::check_bytes32_field(&public_inputs, 12, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        // 4. Run the UltraHonk verification
        Self::verify_proof(env, CircuitType::ShowdownValid, proof, public_inputs)
    }

    // ====================================================================
    // Multi-circuit VK hot-swap by ID
    // ====================================================================

    /// Internal helper to load raw VK entry by circuit ID, falling back to
    /// legacy `CircuitType` storage for default IDs (deal=1, reveal=2, showdown=3).
    fn load_raw_vk_entry(env: &Env, circuit_id: u32) -> Option<VerificationKeyEntry> {
        if let Some(entry) = env
            .storage()
            .persistent()
            .get::<StorageKey, VerificationKeyEntry>(&StorageKey::VkById(circuit_id))
        {
            return Some(entry);
        }
        let legacy_circuit = match circuit_id {
            1 => Some(CircuitType::DealValid),
            2 => Some(CircuitType::RevealBoardValid),
            3 => Some(CircuitType::ShowdownValid),
            _ => None,
        };
        if let Some(c) = legacy_circuit {
            env.storage()
                .persistent()
                .get::<StorageKey, VerificationKeyEntry>(&StorageKey::Vk(c))
        } else {
            None
        }
    }

    /// Admin update path: Hot-swap or register a verification key for a given `circuit_id`.
    ///
    /// Validates VK parseability, enforces monotonic version increments, stores
    /// the entry in persistent storage under `StorageKey::VkById(circuit_id)`,
    /// sets its status to `Active`, and emits a `vk_updated` event.
    pub fn update_vk(
        env: Env,
        admin: Address,
        circuit_id: u32,
        vk_data: Bytes,
        version: u32,
    ) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }

        // Validate the VK can be parsed before storing
        UltraHonkVerifier::new(&env, &vk_data).map_err(|_| VerifierError::VkParseError)?;

        if version == 0 {
            return Err(VerifierError::InvalidVkVersion);
        }

        if let Some(current) = Self::load_raw_vk_entry(&env, circuit_id) {
            if version <= current.version {
                return Err(VerifierError::InvalidVkVersion);
            }
        }

        let entry = VerificationKeyEntry {
            hash: env.crypto().keccak256(&vk_data).into(),
            version,
            activated_at: env.ledger().sequence(),
            vk_data,
        };

        env.storage()
            .persistent()
            .set(&StorageKey::VkById(circuit_id), &entry);

        env.storage()
            .persistent()
            .set(&StorageKey::CircuitStatus(circuit_id), &CircuitStatus::Active);

        env.events().publish(
            (Symbol::new(&env, "vk_updated"), circuit_id),
            (entry.hash.clone(), version, entry.activated_at),
        );
        Ok(())
    }

    /// Alias for `update_vk` to set/hot-swap verification key by circuit ID.
    pub fn set_vk_by_id(
        env: Env,
        admin: Address,
        circuit_id: u32,
        vk_data: Bytes,
        version: u32,
    ) -> Result<(), VerifierError> {
        Self::update_vk(env, admin, circuit_id, vk_data, version)
    }

    /// Mark a verification key / circuit ID as stale (admin only).
    /// Calls with this circuit_id will subsequently return `VerifierError::StaleCircuitId`.
    pub fn mark_vk_stale(
        env: Env,
        admin: Address,
        circuit_id: u32,
    ) -> Result<(), VerifierError> {
        Self::deprecate_vk(env, admin, circuit_id)
    }

    /// Deprecate/retire a circuit ID (admin only).
    pub fn deprecate_vk(
        env: Env,
        admin: Address,
        circuit_id: u32,
    ) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }

        if Self::load_raw_vk_entry(&env, circuit_id).is_none() {
            return Err(VerifierError::UnknownCircuitId);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::CircuitStatus(circuit_id), &CircuitStatus::Stale);

        env.events().publish(
            (Symbol::new(&env, "vk_deprecated"), circuit_id),
            env.ledger().sequence(),
        );
        Ok(())
    }

    /// Set explicit circuit status (admin only): Active, Stale, or Deprecated.
    pub fn set_circuit_status(
        env: Env,
        admin: Address,
        circuit_id: u32,
        status: CircuitStatus,
    ) -> Result<(), VerifierError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(VerifierError::NotInitialized)?;
        if !ct_address_eq(&env, &admin, &stored_admin) {
            return Err(VerifierError::NotAdmin);
        }

        if Self::load_raw_vk_entry(&env, circuit_id).is_none() {
            return Err(VerifierError::UnknownCircuitId);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::CircuitStatus(circuit_id), &status);

        env.events().publish(
            (Symbol::new(&env, "circuit_status_updated"), circuit_id),
            status,
        );
        Ok(())
    }

    /// Lookup verification key entry by circuit ID.
    ///
    /// Returns `Err(VerifierError::UnknownCircuitId)` if ID is unknown.
    /// Returns `Err(VerifierError::StaleCircuitId)` if ID is marked stale/deprecated.
    pub fn get_vk_by_id(
        env: Env,
        circuit_id: u32,
    ) -> Result<VerificationKeyEntry, VerifierError> {
        let entry = Self::load_raw_vk_entry(&env, circuit_id)
            .ok_or(VerifierError::UnknownCircuitId)?;

        let status = env
            .storage()
            .persistent()
            .get(&StorageKey::CircuitStatus(circuit_id))
            .unwrap_or(CircuitStatus::Active);

        if status == CircuitStatus::Stale || status == CircuitStatus::Deprecated {
            return Err(VerifierError::StaleCircuitId);
        }

        Ok(entry)
    }

    /// Alias for `get_vk_by_id`.
    pub fn get_verification_key_by_id(
        env: Env,
        circuit_id: u32,
    ) -> Result<VerificationKeyEntry, VerifierError> {
        Self::get_vk_by_id(env, circuit_id)
    }

    /// Lookup verification key entry by circuit ID and expected version.
    ///
    /// Returns `Err(VerifierError::UnknownCircuitId)` if ID is unknown or version is in the future.
    /// Returns `Err(VerifierError::StaleCircuitId)` if circuit is stale or version < active version.
    pub fn get_vk_by_id_version(
        env: Env,
        circuit_id: u32,
        version: u32,
    ) -> Result<VerificationKeyEntry, VerifierError> {
        let entry = Self::load_raw_vk_entry(&env, circuit_id)
            .ok_or(VerifierError::UnknownCircuitId)?;

        let status = env
            .storage()
            .persistent()
            .get(&StorageKey::CircuitStatus(circuit_id))
            .unwrap_or(CircuitStatus::Active);

        if status == CircuitStatus::Stale || status == CircuitStatus::Deprecated {
            return Err(VerifierError::StaleCircuitId);
        }

        if version < entry.version {
            return Err(VerifierError::StaleCircuitId);
        }
        if version > entry.version {
            return Err(VerifierError::UnknownCircuitId);
        }

        Ok(entry)
    }

    /// Get current circuit status: Active, Stale, or Deprecated.
    pub fn get_circuit_status(
        env: Env,
        circuit_id: u32,
    ) -> Result<CircuitStatus, VerifierError> {
        if Self::load_raw_vk_entry(&env, circuit_id).is_none() {
            return Err(VerifierError::UnknownCircuitId);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&StorageKey::CircuitStatus(circuit_id))
            .unwrap_or(CircuitStatus::Active))
    }

    /// Returns true if circuit ID exists and is currently active.
    pub fn is_circuit_active(env: Env, circuit_id: u32) -> bool {
        match Self::get_vk_by_id(env, circuit_id) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Returns true if circuit ID exists and is stale/deprecated.
    pub fn is_circuit_stale(env: Env, circuit_id: u32) -> bool {
        if Self::load_raw_vk_entry(&env, circuit_id).is_none() {
            return false;
        }
        let status = env
            .storage()
            .persistent()
            .get(&StorageKey::CircuitStatus(circuit_id))
            .unwrap_or(CircuitStatus::Active);
        status == CircuitStatus::Stale || status == CircuitStatus::Deprecated
    }

    /// Read full `CircuitEntry` containing id, VK, and status.
    pub fn get_circuit_entry(
        env: Env,
        circuit_id: u32,
    ) -> Result<CircuitEntry, VerifierError> {
        let vk = Self::load_raw_vk_entry(&env, circuit_id)
            .ok_or(VerifierError::UnknownCircuitId)?;
        let status = env
            .storage()
            .persistent()
            .get(&StorageKey::CircuitStatus(circuit_id))
            .unwrap_or(CircuitStatus::Active);
        Ok(CircuitEntry {
            circuit_id,
            vk,
            status,
        })
    }

    /// Verify an UltraHonk proof using VK looked up by circuit ID.
    ///
    /// 1. Rejects if contract is paused (`ContractPaused`)
    /// 2. Validates proof size (`ProofSizeError`)
    /// 3. Looks up VK by ID (`UnknownCircuitId` or `StaleCircuitId`)
    /// 4. Verifies proof with UltraHonk
    /// 5. Stores proof hash in persistent storage
    pub fn verify_proof_by_id(
        env: Env,
        circuit_id: u32,
        proof: Bytes,
        public_inputs: Bytes,
    ) -> Result<bool, VerifierError> {
        if env
            .storage()
            .instance()
            .get::<StorageKey, bool>(&StorageKey::Paused)
            .unwrap_or(false)
        {
            return Err(VerifierError::ContractPaused);
        }

        if proof.len() as usize != PROOF_BYTES {
            return Err(VerifierError::ProofSizeError);
        }

        let vk_entry = Self::get_vk_by_id(env.clone(), circuit_id)?;

        let verifier = UltraHonkVerifier::new(&env, &vk_entry.vk_data)
            .map_err(|_| VerifierError::VkParseError)?;

        verifier
            .verify(&proof, &public_inputs)
            .map_err(|_| VerifierError::VerificationFailed)?;

        let proof_hash = env.crypto().keccak256(&proof);
        env.storage()
            .persistent()
            .set(&StorageKey::ProofVerified(proof_hash.clone().into()), &true);

        env.events().publish(
            (Symbol::new(&env, "proof_verified_by_id"), circuit_id),
            proof_hash,
        );

        Ok(true)
    }

    /// Verify a deal proof by circuit ID.
    pub fn verify_deal_by_id(
        env: Env,
        circuit_id: u32,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        hand_commitments: Vec<BytesN<32>>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != DEAL_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        if hand_commitments.len() > MAX_PLAYERS {
            return Err(VerifierError::WrongCommitmentCount);
        }

        if !Self::check_bytes32_field(&public_inputs, 1, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        for i in 0..hand_commitments.len() {
            let expected = hand_commitments
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_bytes32_field(&public_inputs, 2 + i, &expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        Self::verify_proof_by_id(env, circuit_id, proof, public_inputs)
    }

    /// Verify a board reveal proof by circuit ID.
    pub fn verify_reveal_by_id(
        env: Env,
        circuit_id: u32,
        proof: Bytes,
        public_inputs: Bytes,
        deck_root: BytesN<32>,
        revealed_cards: Vec<u32>,
        revealed_indices: Vec<u32>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != REVEAL_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        let num_revealed = revealed_cards.len();
        if num_revealed != revealed_indices.len() || num_revealed > 3 {
            return Err(VerifierError::PublicInputMismatch);
        }

        if !Self::check_bytes32_field(&public_inputs, 0, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        for i in 0..num_revealed {
            let expected = revealed_cards
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 19 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        for i in 0..num_revealed {
            let expected = revealed_indices
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 22 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        Self::verify_proof_by_id(env, circuit_id, proof, public_inputs)
    }

    /// Verify a showdown proof by circuit ID.
    pub fn verify_showdown_by_id(
        env: Env,
        circuit_id: u32,
        proof: Bytes,
        public_inputs: Bytes,
        hand_commitments: Vec<BytesN<32>>,
        board_indices: Vec<u32>,
        deck_root: BytesN<32>,
    ) -> Result<bool, VerifierError> {
        if public_inputs.len() != SHOWDOWN_BYTES {
            return Err(VerifierError::PublicInputSizeError);
        }
        if hand_commitments.len() > MAX_PLAYERS {
            return Err(VerifierError::WrongCommitmentCount);
        }
        if board_indices.len() != BOARD_INDICES_COUNT {
            return Err(VerifierError::WrongBoardIndicesCount);
        }

        for i in 0..hand_commitments.len() {
            let expected = hand_commitments
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_bytes32_field(&public_inputs, 1 + i, &expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        for i in 0..BOARD_INDICES_COUNT {
            let expected = board_indices
                .get(i)
                .ok_or(VerifierError::PublicInputMismatch)?;
            if !Self::check_u32_field(&public_inputs, 7 + i, expected) {
                return Err(VerifierError::PublicInputMismatch);
            }
        }

        if !Self::check_bytes32_field(&public_inputs, 12, &deck_root) {
            return Err(VerifierError::PublicInputMismatch);
        }

        Self::verify_proof_by_id(env, circuit_id, proof, public_inputs)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        vec, Address, Bytes, Env,
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

    #[test]
    fn test_pause_and_unpause() {
        let (_env, client, admin) = setup();
        assert!(!client.is_paused());
        client.pause(&admin);
        assert!(client.is_paused());
        client.unpause(&admin);
        assert!(!client.is_paused());
    }

    #[test]
    fn test_paused_blocks_verify_proof() {
        let (env, client, admin) = setup();
        client.pause(&admin);

        let proof = Bytes::new(&env);
        let public_inputs = Bytes::new(&env);
        let result = client.try_verify_proof(&CircuitType::DealValid, &proof, &public_inputs);
        assert_eq!(result, Err(Ok(VerifierError::ContractPaused)));
    }

    #[test]
    fn test_non_admin_cannot_pause() {
        let (env, client, _admin) = setup();
        let stranger = Address::generate(&env);
        let result = client.try_pause(&stranger);
        assert_eq!(result, Err(Ok(VerifierError::NotAdmin)));
    }

    #[test]
    fn test_admin_can_set_vk_while_paused() {
        let (env, client, admin) = setup();
        client.pause(&admin);
        // set_verification_key is admin-only and must work while paused
        // (VK parsing will fail on empty bytes but should not return ContractPaused)
        let vk = Bytes::new(&env);
        let result = client.try_set_verification_key(&admin, &CircuitType::DealValid, &vk, &1);
        assert!(matches!(result, Err(Ok(VerifierError::VkParseError))));
    }

    // ------------------------------------------------------------------
    // Upgrade governance (Issue #504)
    // ------------------------------------------------------------------

    const GOV_DELAY: u32 = 100;

    fn wasm_hash(env: &Env, tag: u8) -> BytesN<32> {
        let mut arr = [tag; 32];
        arr[0] = tag;
        BytesN::from_array(env, &arr)
    }

    #[test]
    fn test_governance_threshold_and_timelock() {
        let (env, client, admin) = setup();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        client.configure_upgrade_governance(&admin, &vec![&env, s1.clone(), s2.clone()], &2, &GOV_DELAY);

        let target = wasm_hash(&env, 1);
        // Signer 1 proposes; threshold (2) not yet reached.
        client.propose_upgrade(&s1, &target);
        let approvals = client.get_pending_upgrade();
        assert_eq!(approvals.approvals.len(), 1);
        assert_eq!(
            client.try_can_execute_upgrade(),
            Err(Ok(VerifierError::NotEnoughUpgradeApprovals))
        );

        // Signer 2 approves; timelock not yet elapsed.
        client.propose_upgrade(&s2, &target);
        env.ledger().set_sequence_number(1);
        assert_eq!(
            client.try_can_execute_upgrade(),
            Err(Ok(VerifierError::UpgradeTimelockPending))
        );

        // Once approved AND past the delay the upgrade is executable.
        env.ledger().set_sequence_number(GOV_DELAY + 1);
        assert_eq!(client.try_can_execute_upgrade(), Ok(Ok(())));
    }

    #[test]
    fn test_governance_rejects_non_signer_and_bad_config() {
        let (env, client, admin) = setup();
        let s1 = Address::generate(&env);
        client.configure_upgrade_governance(&admin, &vec![&env, s1.clone()], &1, &GOV_DELAY);

        let stranger = Address::generate(&env);
        let target = wasm_hash(&env, 2);
        assert_eq!(
            client.try_propose_upgrade(&stranger, &target),
            Err(Ok(VerifierError::NotAnUpgradeSigner))
        );

        // Single signer may not run before the timelock (threshold met but delay pending).
        client.propose_upgrade(&s1, &target);
        env.ledger().set_sequence_number(GOV_DELAY - 1);
        assert_eq!(
            client.try_can_execute_upgrade(),
            Err(Ok(VerifierError::UpgradeTimelockPending))
        );
        env.ledger().set_sequence_number(GOV_DELAY + 1);
        assert_eq!(client.try_can_execute_upgrade(), Ok(Ok(())));
    }

    #[test]
    fn test_governance_duplicate_approval_rejected() {
        let (env, client, admin) = setup();
        let s1 = Address::generate(&env);
        client.configure_upgrade_governance(&admin, &vec![&env, s1.clone()], &1, &GOV_DELAY);

        let target = wasm_hash(&env, 3);
        client.propose_upgrade(&s1, &target);
        assert_eq!(
            client.try_propose_upgrade(&s1, &target),
            Err(Ok(VerifierError::UpgradeAlreadyApproved))
        );
    }

    #[test]
    fn test_governance_config_validation() {
        let (env, client, admin) = setup();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);

        // Empty signers.
        assert_eq!(
            client.try_configure_upgrade_governance(&admin, &Vec::new(&env), &1, &GOV_DELAY),
            Err(Ok(VerifierError::InvalidGovernanceConfig))
        );
        // Zero threshold.
        assert_eq!(
            client.try_configure_upgrade_governance(
                &admin,
                &vec![&env, s1.clone(), s2.clone()],
                &0,
                &GOV_DELAY
            ),
            Err(Ok(VerifierError::InvalidGovernanceConfig))
        );
        // Threshold exceeds set size.
        assert_eq!(
            client.try_configure_upgrade_governance(
                &admin,
                &vec![&env, s1.clone(), s2.clone()],
                &3,
                &GOV_DELAY
            ),
            Err(Ok(VerifierError::InvalidGovernanceConfig))
        );
        // Zero timelock delay.
        assert_eq!(
            client.try_configure_upgrade_governance(
                &admin,
                &vec![&env, s1.clone(), s2.clone()],
                &2,
                &0
            ),
            Err(Ok(VerifierError::InvalidGovernanceConfig))
        );
        // Duplicate signer.
        assert_eq!(
            client.try_configure_upgrade_governance(&admin, &vec![&env, s1.clone(), s1], &2, &GOV_DELAY),
            Err(Ok(VerifierError::InvalidGovernanceConfig))
        );
    }

    #[test]
    fn test_governance_admin_only() {
        let (env, client, _admin) = setup();
        let stranger = Address::generate(&env);
        let s1 = Address::generate(&env);
        assert_eq!(
            client.try_configure_upgrade_governance(&stranger, &vec![&env, s1], &1, &GOV_DELAY),
            Err(Ok(VerifierError::NotAdmin))
        );
    }

    // ------------------------------------------------------------------
    // Multi-circuit VK hot-swap tests
    // ------------------------------------------------------------------

    fn dummy_vk(env: &Env, tag: u8) -> Bytes {
        let mut arr = [0u8; 1824];
        arr[0] = tag;
        Bytes::from_slice(env, &arr)
    }

    #[test]
    fn test_unknown_id() {
        let (env, client, _admin) = setup();
        let unknown_id = 999;

        // get_vk_by_id fails with UnknownCircuitId
        let res = client.try_get_vk_by_id(&unknown_id);
        assert_eq!(res, Err(Ok(VerifierError::UnknownCircuitId)));

        // get_verification_key_by_id alias also fails with UnknownCircuitId
        let res_alias = client.try_get_verification_key_by_id(&unknown_id);
        assert_eq!(res_alias, Err(Ok(VerifierError::UnknownCircuitId)));

        // get_circuit_status fails with UnknownCircuitId
        let status_res = client.try_get_circuit_status(&unknown_id);
        assert_eq!(status_res, Err(Ok(VerifierError::UnknownCircuitId)));

        // get_circuit_entry fails with UnknownCircuitId
        let entry_res = client.try_get_circuit_entry(&unknown_id);
        assert_eq!(entry_res, Err(Ok(VerifierError::UnknownCircuitId)));

        // is_circuit_active returns false, is_circuit_stale returns false
        assert!(!client.is_circuit_active(&unknown_id));
        assert!(!client.is_circuit_stale(&unknown_id));

        // verify_proof_by_id fails with UnknownCircuitId
        let proof = Bytes::from_slice(&env, &[0u8; PROOF_BYTES]);
        let inputs = Bytes::new(&env);
        let verify_res = client.try_verify_proof_by_id(&unknown_id, &proof, &inputs);
        assert_eq!(verify_res, Err(Ok(VerifierError::UnknownCircuitId)));
    }

    #[test]
    fn test_stale_id() {
        let (env, client, admin) = setup();
        let circuit_id = 100;
        let vk_data = dummy_vk(&env, 42);

        // Admin registers VK for circuit_id 100 with version 1
        client.update_vk(&admin, &circuit_id, &vk_data, &1);
        assert!(client.is_circuit_active(&circuit_id));
        assert!(!client.is_circuit_stale(&circuit_id));

        // Lookup works
        let entry = client.get_vk_by_id(&circuit_id);
        assert_eq!(entry.version, 1);

        // Admin marks VK as stale
        client.mark_vk_stale(&admin, &circuit_id);
        assert!(!client.is_circuit_active(&circuit_id));
        assert!(client.is_circuit_stale(&circuit_id));
        assert_eq!(
            client.get_circuit_status(&circuit_id),
            CircuitStatus::Stale
        );

        // get_vk_by_id fails with StaleCircuitId
        let res = client.try_get_vk_by_id(&circuit_id);
        assert_eq!(res, Err(Ok(VerifierError::StaleCircuitId)));

        // get_verification_key_by_id alias also fails with StaleCircuitId
        let res_alias = client.try_get_verification_key_by_id(&circuit_id);
        assert_eq!(res_alias, Err(Ok(VerifierError::StaleCircuitId)));

        // verify_proof_by_id fails with StaleCircuitId
        let proof = Bytes::from_slice(&env, &[0u8; PROOF_BYTES]);
        let inputs = Bytes::new(&env);
        let verify_res = client.try_verify_proof_by_id(&circuit_id, &proof, &inputs);
        assert_eq!(verify_res, Err(Ok(VerifierError::StaleCircuitId)));

        // get_vk_by_id_version with older version returns StaleCircuitId
        let ver_res = client.try_get_vk_by_id_version(&circuit_id, &0);
        assert_eq!(ver_res, Err(Ok(VerifierError::StaleCircuitId)));
    }

    #[test]
    fn test_admin_update_path() {
        let (env, client, admin) = setup();
        let stranger = Address::generate(&env);
        let circuit_id = 200;
        let vk = dummy_vk(&env, 5);

        // Stranger cannot update VK
        let res_stranger = client.try_update_vk(&stranger, &circuit_id, &vk, &1);
        assert_eq!(res_stranger, Err(Ok(VerifierError::NotAdmin)));

        // Stranger cannot mark VK stale
        let res_stale_stranger = client.try_mark_vk_stale(&stranger, &circuit_id);
        assert_eq!(res_stale_stranger, Err(Ok(VerifierError::NotAdmin)));

        // Admin updates VK successfully
        client.update_vk(&admin, &circuit_id, &vk, &1);
        let entry = client.get_vk_by_id(&circuit_id);
        assert_eq!(entry.version, 1);
        assert_eq!(client.get_circuit_status(&circuit_id), CircuitStatus::Active);

        // Monotonic version: updating with same or lower version fails with InvalidVkVersion
        let res_same_ver = client.try_update_vk(&admin, &circuit_id, &vk, &1);
        assert_eq!(res_same_ver, Err(Ok(VerifierError::InvalidVkVersion)));

        let res_zero_ver = client.try_update_vk(&admin, &circuit_id, &vk, &0);
        assert_eq!(res_zero_ver, Err(Ok(VerifierError::InvalidVkVersion)));

        // Hot-swap with version 2 succeeds
        let vk_v2 = dummy_vk(&env, 6);
        client.update_vk(&admin, &circuit_id, &vk_v2, &2);
        let entry_v2 = client.get_vk_by_id(&circuit_id);
        assert_eq!(entry_v2.version, 2);
    }

    #[test]
    fn test_stale_id_reactivation_via_hot_swap() {
        let (env, client, admin) = setup();
        let circuit_id = 101;
        let vk_v1 = dummy_vk(&env, 1);
        let vk_v2 = dummy_vk(&env, 2);

        client.update_vk(&admin, &circuit_id, &vk_v1, &1);
        client.mark_vk_stale(&admin, &circuit_id);
        assert_eq!(
            client.try_get_vk_by_id(&circuit_id),
            Err(Ok(VerifierError::StaleCircuitId))
        );

        // Hot-swap with new version 2 reactivates the circuit
        client.update_vk(&admin, &circuit_id, &vk_v2, &2);
        assert!(client.is_circuit_active(&circuit_id));
        let entry = client.get_vk_by_id(&circuit_id);
        assert_eq!(entry.version, 2);
    }

    #[test]
    fn test_multi_circuit_hot_swap_multiple_circuits() {
        let (env, client, admin) = setup();

        // Register multiple circuits: deal(1), reveal(2), showdown(3), omaha(4), short_deck(5)
        let vk1 = dummy_vk(&env, 1);
        let vk2 = dummy_vk(&env, 2);
        let vk3 = dummy_vk(&env, 3);
        let vk4 = dummy_vk(&env, 4);
        let vk5 = dummy_vk(&env, 5);

        client.update_vk(&admin, &1, &vk1, &1);
        client.update_vk(&admin, &2, &vk2, &1);
        client.update_vk(&admin, &3, &vk3, &1);
        client.update_vk(&admin, &4, &vk4, &1);
        client.update_vk(&admin, &5, &vk5, &1);

        // Verify independent lookups
        assert_eq!(client.get_vk_by_id(&1).version, 1);
        assert_eq!(client.get_vk_by_id(&2).version, 1);
        assert_eq!(client.get_vk_by_id(&3).version, 1);
        assert_eq!(client.get_vk_by_id(&4).version, 1);
        assert_eq!(client.get_vk_by_id(&5).version, 1);

        // Hot-swap variant 4 to version 2
        let vk4_v2 = dummy_vk(&env, 42);
        client.update_vk(&admin, &4, &vk4_v2, &2);
        assert_eq!(client.get_vk_by_id(&4).version, 2);

        // Others unaffected
        assert_eq!(client.get_vk_by_id(&1).version, 1);
        assert_eq!(client.get_vk_by_id(&2).version, 1);
        assert_eq!(client.get_vk_by_id(&3).version, 1);
        assert_eq!(client.get_vk_by_id(&5).version, 1);

        // Deprecate circuit 2
        client.deprecate_vk(&admin, &2);
        assert_eq!(client.try_get_vk_by_id(&2), Err(Ok(VerifierError::StaleCircuitId)));
        assert_eq!(client.get_vk_by_id(&1).version, 1);
        assert_eq!(client.get_vk_by_id(&3).version, 1);
        assert_eq!(client.get_vk_by_id(&4).version, 2);
        assert_eq!(client.get_vk_by_id(&5).version, 1);
    }

    #[test]
    fn test_legacy_circuit_fallback() {
        let (env, client, admin) = setup();
        let vk = dummy_vk(&env, 10);
        client.set_verification_key(&admin, &CircuitType::DealValid, &vk, &1);

        // Querying via circuit_id 1 resolves legacy DealValid key
        let entry = client.get_vk_by_id(&1);
        assert_eq!(entry.version, 1);

        // Hot-swap with update_vk overrides legacy
        let vk_v2 = dummy_vk(&env, 11);
        client.update_vk(&admin, &1, &vk_v2, &2);
        let entry_v2 = client.get_vk_by_id(&1);
        assert_eq!(entry_v2.version, 2);
    }

    #[test]
    fn test_paused_blocks_verify_proof_by_id() {
        let (env, client, admin) = setup();
        let circuit_id = 300;
        let vk = dummy_vk(&env, 12);
        client.update_vk(&admin, &circuit_id, &vk, &1);

        client.pause(&admin);
        let proof = Bytes::from_slice(&env, &[0u8; PROOF_BYTES]);
        let inputs = Bytes::new(&env);
        let res = client.try_verify_proof_by_id(&circuit_id, &proof, &inputs);
        assert_eq!(res, Err(Ok(VerifierError::ContractPaused)));

        client.unpause(&admin);
        // While unpaused, fails on verification rather than ContractPaused
        let res_unpaused = client.try_verify_proof_by_id(&circuit_id, &proof, &inputs);
        assert_ne!(res_unpaused, Err(Ok(VerifierError::ContractPaused)));
    }

    #[test]
    fn test_admin_can_update_vk_while_paused() {
        let (env, client, admin) = setup();
        client.pause(&admin);
        let circuit_id = 301;
        let vk = dummy_vk(&env, 13);
        // Admin update path must work while paused for emergency hot-swaps
        let res = client.try_update_vk(&admin, &circuit_id, &vk, &1);
        assert_eq!(res, Ok(Ok(())));
    }
}
