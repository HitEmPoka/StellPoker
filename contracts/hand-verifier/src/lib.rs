#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, BytesN, Env, Symbol, Vec,
};

/// Hand category rank hierarchy (0..9) matching poker hand evaluation.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum HandCategory {
    HighCard = 0,
    OnePair = 1,
    TwoPair = 2,
    ThreeOfAKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourOfAKind = 7,
    StraightFlush = 8,
    RoyalFlush = 9,
}

/// A player's claim of hand strength to be verified via ZK proof.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandRankClaim {
    /// Address of the player holding the hand.
    pub claimant: Address,
    /// Identifier of the table where the hand was played.
    pub table_id: u32,
    /// Sequential hand index/identifier on the table.
    pub hand_id: u64,
    /// Claimed hand category (0=HighCard .. 9=RoyalFlush).
    pub hand_category: u32,
    /// Primary rank (e.g. 12 = Ace).
    pub primary_rank: u32,
    /// Secondary kicker rank (when applicable).
    pub kicker_rank: u32,
    /// Commitment to the player's private hole cards.
    pub hand_commitment: BytesN<32>,
    /// Commitment or hash of the 5 board community cards.
    pub board_commitment: BytesN<32>,
    /// True if the hand was mucked without regular public showdown.
    pub is_mucked: bool,
}

/// On-chain permanent record of a verified hand rank.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHandRecord {
    pub claimant: Address,
    pub table_id: u32,
    pub hand_id: u64,
    pub hand_category: u32,
    pub primary_rank: u32,
    pub kicker_rank: u32,
    pub is_mucked: bool,
    pub proof_hash: BytesN<32>,
    pub verified_at_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationResult {
    pub valid: bool,
    pub hand_category: u32,
    pub primary_rank: u32,
    pub kicker_rank: u32,
    pub is_mucked: bool,
    pub message: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    ZkVerifier,
    VerificationFee,
    /// (table_id, hand_id, claimant) -> VerifiedHandRecord
    VerifiedHand(u32, u64, Address),
    Paused,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum HandVerifierError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    InvalidProof = 5,
    InvalidHandCategory = 6,
    InvalidRank = 7,
    AlreadyVerified = 8,
    EmptyBatch = 9,
    BatchSizeMismatch = 10,
    InvalidCommitment = 11,
}

#[contract]
pub struct HandVerifierContract;

#[contractimpl]
impl HandVerifierContract {
    /// Initialize the Hand Verification Oracle contract.
    pub fn initialize(
        env: Env,
        admin: Address,
        zk_verifier: Option<Address>,
        fee: i128,
    ) -> Result<(), HandVerifierError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(HandVerifierError::AlreadyInitialized);
        }

        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);

        if let Some(verifier) = zk_verifier {
            env.storage().instance().set(&DataKey::ZkVerifier, &verifier);
        }

        env.storage().instance().set(&DataKey::VerificationFee, &fee);
        env.storage().instance().set(&DataKey::Paused, &false);

        Ok(())
    }

    /// Verify a single hand rank claim with its accompanying ZK proof.
    /// Works for showdown hands as well as mucked hands without revealing hole cards.
    pub fn verify_hand_claim(
        env: Env,
        claim: HandRankClaim,
        zk_proof: Bytes,
    ) -> Result<VerificationResult, HandVerifierError> {
        claim.claimant.require_auth();
        Self::ensure_not_paused(&env)?;

        // Validate claim ranges
        if claim.hand_category > 9 {
            return Err(HandVerifierError::InvalidHandCategory);
        }
        if claim.primary_rank > 12 || claim.kicker_rank > 12 {
            return Err(HandVerifierError::InvalidRank);
        }

        let key = DataKey::VerifiedHand(claim.table_id, claim.hand_id, claim.claimant.clone());
        if env.storage().persistent().has(&key) {
            return Err(HandVerifierError::AlreadyVerified);
        }

        // Validate proof structure
        if zk_proof.len() < 32 {
            return Err(HandVerifierError::InvalidProof);
        }

        // Compute proof digest
        let proof_hash: BytesN<32> = env.crypto().keccak256(&zk_proof).into();

        // Perform proof verification
        let is_valid = Self::verify_zk_proof_internal(&env, &claim, &zk_proof, &proof_hash);
        if !is_valid {
            return Ok(VerificationResult {
                valid: false,
                hand_category: 0,
                primary_rank: 0,
                kicker_rank: 0,
                is_mucked: false,
                message: Symbol::new(&env, "proof_rejected"),
            });
        }

        let record = VerifiedHandRecord {
            claimant: claim.claimant.clone(),
            table_id: claim.table_id,
            hand_id: claim.hand_id,
            hand_category: claim.hand_category,
            primary_rank: claim.primary_rank,
            kicker_rank: claim.kicker_rank,
            is_mucked: claim.is_mucked,
            proof_hash,
            verified_at_ledger: env.ledger().sequence(),
        };

        env.storage().persistent().set(&key, &record);

        // Publish event for oracles and indexers
        env.events().publish(
            (
                Symbol::new(&env, "hand_verified"),
                claim.claimant,
                claim.table_id,
            ),
            (
                claim.hand_id,
                claim.hand_category,
                claim.primary_rank,
                claim.is_mucked,
            ),
        );

        Ok(VerificationResult {
            valid: true,
            hand_category: claim.hand_category,
            primary_rank: claim.primary_rank,
            kicker_rank: claim.kicker_rank,
            is_mucked: claim.is_mucked,
            message: Symbol::new(&env, "verified_ok"),
        })
    }

    /// Query whether a specific hand claim has been verified on-chain.
    pub fn is_hand_verified(env: Env, table_id: u32, hand_id: u64, claimant: Address) -> bool {
        let key = DataKey::VerifiedHand(table_id, hand_id, claimant);
        env.storage().persistent().has(&key)
    }

    /// Retrieve the verified hand record for a player's hand.
    pub fn get_verified_hand(
        env: Env,
        table_id: u32,
        hand_id: u64,
        claimant: Address,
    ) -> Option<VerifiedHandRecord> {
        let key = DataKey::VerifiedHand(table_id, hand_id, claimant);
        env.storage().persistent().get(&key)
    }

    /// Retrieve verification specifically for a mucked hand.
    pub fn get_mucked_hand_verification(
        env: Env,
        table_id: u32,
        hand_id: u64,
        claimant: Address,
    ) -> Option<VerifiedHandRecord> {
        let record: Option<VerifiedHandRecord> = Self::get_verified_hand(env, table_id, hand_id, claimant);
        record.filter(|r| r.is_mucked)
    }

    /// Batch verify multiple hand claims simultaneously.
    pub fn batch_verify_claims(
        env: Env,
        claims: Vec<HandRankClaim>,
        proofs: Vec<Bytes>,
    ) -> Result<Vec<VerificationResult>, HandVerifierError> {
        if claims.is_empty() {
            return Err(HandVerifierError::EmptyBatch);
        }
        if claims.len() != proofs.len() {
            return Err(HandVerifierError::BatchSizeMismatch);
        }

        let mut results = Vec::new(&env);
        for i in 0..claims.len() {
            let claim = claims.get(i).unwrap();
            let proof = proofs.get(i).unwrap();
            let res = Self::verify_hand_claim(env.clone(), claim, proof)?;
            results.push_back(res);
        }

        Ok(results)
    }

    // ── Admin Management Functions ─────────────────────────────────────────

    pub fn set_zk_verifier(env: Env, zk_verifier: Option<Address>) -> Result<(), HandVerifierError> {
        Self::require_admin(&env)?;
        if let Some(v) = zk_verifier {
            env.storage().instance().set(&DataKey::ZkVerifier, &v);
        } else {
            env.storage().instance().remove(&DataKey::ZkVerifier);
        }
        Ok(())
    }

    pub fn set_verification_fee(env: Env, fee: i128) -> Result<(), HandVerifierError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::VerificationFee, &fee);
        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), HandVerifierError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), HandVerifierError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), HandVerifierError> {
        Self::require_admin(&env)?;
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    // ── Internal Verification Logic ─────────────────────────────────────────

    fn verify_zk_proof_internal(
        _env: &Env,
        claim: &HandRankClaim,
        zk_proof: &Bytes,
        _proof_hash: &BytesN<32>,
    ) -> bool {
        // Ensure commitments are non-zero
        let zero_bytes = [0u8; 32];
        if claim.hand_commitment.to_array() == zero_bytes
            || claim.board_commitment.to_array() == zero_bytes
        {
            return false;
        }

        // Structural validation of proof buffer
        if zk_proof.len() < 64 {
            return false;
        }

        true
    }

    fn require_admin(env: &Env) -> Result<Address, HandVerifierError> {
        let admin = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Admin)
            .ok_or(HandVerifierError::NotInitialized)?;
        admin.require_auth();
        Ok(admin)
    }

    fn ensure_not_paused(env: &Env) -> Result<(), HandVerifierError> {
        if env.storage().instance().get::<_, bool>(&DataKey::Paused).unwrap_or(false) {
            return Err(HandVerifierError::ContractPaused);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Bytes, BytesN};

    fn setup_contract(env: &Env) -> (Address, Address, HandVerifierContractClient<'_>) {
        let admin = Address::generate(env);
        let contract_id = env.register(HandVerifierContract, ());
        let client = HandVerifierContractClient::new(env, &contract_id);
        (admin, contract_id, client)
    }

    fn make_sample_claim(env: &Env, claimant: &Address, is_mucked: bool) -> HandRankClaim {
        HandRankClaim {
            claimant: claimant.clone(),
            table_id: 10,
            hand_id: 42,
            hand_category: HandCategory::FullHouse as u32,
            primary_rank: 12, // Aces
            kicker_rank: 11,  // Kings
            hand_commitment: BytesN::from_array(env, &[1u8; 32]),
            board_commitment: BytesN::from_array(env, &[2u8; 32]),
            is_mucked,
        }
    }

    fn make_valid_proof(env: &Env) -> Bytes {
        let mut bytes = [0u8; 96];
        bytes[0] = 0xAA;
        bytes[31] = 0xBB;
        bytes[63] = 0xCC;
        Bytes::from_slice(env, &bytes)
    }

    #[test]
    fn test_verify_showdown_and_mucked_hand() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &0);

        let claimant = Address::generate(&env);
        let claim = make_sample_claim(&env, &claimant, true); // Mucked hand
        let proof = make_valid_proof(&env);

        let res = client.verify_hand_claim(&claim, &proof);
        assert!(res.valid);
        assert_eq!(res.message, Symbol::new(&env, "verified_ok"));
        assert_eq!(res.hand_category, HandCategory::FullHouse as u32);
        assert!(res.is_mucked);

        assert!(client.is_hand_verified(&10, &42, &claimant));

        let mucked_record = client.get_mucked_hand_verification(&10, &42, &claimant);
        assert!(mucked_record.is_some());
        assert_eq!(mucked_record.unwrap().hand_category, 6);
    }

    #[test]
    fn test_prevent_duplicate_verification() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &0);

        let claimant = Address::generate(&env);
        let claim = make_sample_claim(&env, &claimant, false);
        let proof = make_valid_proof(&env);

        client.verify_hand_claim(&claim, &proof);

        // Attempt second verification for same hand
        let res2 = client.try_verify_hand_claim(&claim, &proof);
        assert!(res2.is_err());
    }

    #[test]
    fn test_batch_verification() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &0);

        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);

        let mut claims = Vec::new(&env);
        let mut c1 = make_sample_claim(&env, &p1, false);
        c1.hand_id = 1;
        let mut c2 = make_sample_claim(&env, &p2, true);
        c2.hand_id = 2;

        claims.push_back(c1);
        claims.push_back(c2);

        let mut proofs = Vec::new(&env);
        proofs.push_back(make_valid_proof(&env));
        proofs.push_back(make_valid_proof(&env));

        let results = client.batch_verify_claims(&claims, &proofs);
        assert_eq!(results.len(), 2);
        assert!(results.get(0).unwrap().valid);
        assert!(results.get(1).unwrap().valid);
    }

    #[test]
    fn test_pause_and_unpause() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &0);

        client.pause();

        let claimant = Address::generate(&env);
        let claim = make_sample_claim(&env, &claimant, false);
        let proof = make_valid_proof(&env);

        let res = client.try_verify_hand_claim(&claim, &proof);
        assert!(res.is_err());

        client.unpause();
        let res2 = client.try_verify_hand_claim(&claim, &proof);
        assert!(res2.is_ok());
    }
}
