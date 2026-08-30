#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, BytesN, Env, String,
    Symbol,
};

pub const BPS_SCALE: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RakePayload {
    pub default_rake_bps: u32,
    pub max_rake_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitteePayload {
    pub min_stake: i128,
    pub reward_rate_bps: u32,
    pub slash_penalty_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitPayload {
    pub new_verifier: Address,
    pub new_circuit_hash: BytesN<32>,
    pub description: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableLimitsPayload {
    pub min_buy_in_floor: i128,
    pub max_players_limit: u32,
    pub min_action_timeout_seconds: u32,
    pub max_rebuys_limit: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalAction {
    /// 1. Vote on protocol rake percentage and maximum cap
    SetRakePercentage(RakePayload),
    /// 2. Vote on MPC committee rewards and slashing parameters
    SetCommitteeRewards(CommitteePayload),
    /// 3. Vote on ZK circuit verifier contract and circuit wasm/vkey hash
    ProposeCircuitUpgrade(CircuitPayload),
    /// 4. Vote on global table configuration limits (buy-ins, max players, timeouts)
    SetTableConfigLimits(TableLimitsPayload),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Active,
    Defeated,
    Succeeded,
    Queued,
    Executed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteType {
    Against = 0,
    For = 1,
    Abstain = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceConfig {
    pub proposal_threshold: i128,     // Min staked tokens to submit proposal
    pub voting_delay_ledgers: u32,   // Delay before voting starts
    pub voting_period_ledgers: u32,  // Duration of voting
    pub timelock_delay_ledgers: u32, // Delay after passing before execution
    pub quorum_bps: u32,             // Quorum requirement (e.g. 400 = 4%)
    pub approval_threshold_bps: u32, // Majority requirement (e.g. 5000 = 50%)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub action: ProposalAction,
    pub title: String,
    pub description: String,
    pub start_ledger: u32,
    pub end_ledger: u32,
    pub execution_after_ledger: u32,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub status: ProposalStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveProtocolParams {
    pub default_rake_bps: u32,
    pub max_rake_bps: u32,
    pub committee_min_stake: i128,
    pub committee_reward_rate_bps: u32,
    pub committee_slash_penalty_bps: u32,
    pub circuit_verifier: Address,
    pub circuit_hash: BytesN<32>,
    pub table_min_buy_in_floor: i128,
    pub table_max_players_limit: u32,
    pub table_min_timeout_seconds: u32,
    pub table_max_rebuys_limit: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    GovernanceToken,
    Config,
    NextProposalId,
    TotalStaked,
    StakedBalance(Address),
    Proposal(u32),
    HasVoted(u32, Address),
    ActiveParams,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GovernanceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InsufficientVotingPower = 4,
    ProposalNotFound = 5,
    ProposalNotActive = 6,
    AlreadyVoted = 7,
    VotingNotEnded = 8,
    QuorumNotMet = 9,
    ProposalNotPassed = 10,
    TimelockNotElapsed = 11,
    ProposalAlreadyExecuted = 12,
    InvalidStatus = 13,
    InvalidConfig = 14,
    InvalidAmount = 15,
}

#[contract]
pub struct ProtocolGovernanceContract;

#[contractimpl]
impl ProtocolGovernanceContract {
    /// Initialize Protocol Governance with STELLPOKER token, admin, and initial parameters
    pub fn initialize(
        env: Env,
        admin: Address,
        governance_token: Address,
        config: GovernanceConfig,
        initial_params: ActiveProtocolParams,
    ) -> Result<(), GovernanceError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(GovernanceError::AlreadyInitialized);
        }

        if config.proposal_threshold <= 0 || config.quorum_bps > 10_000 || config.approval_threshold_bps > 10_000 {
            return Err(GovernanceError::InvalidConfig);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::GovernanceToken, &governance_token);
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &1u32);
        env.storage().instance().set(&DataKey::TotalStaked, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::ActiveParams, &initial_params);
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "gov_initialized"),),
            (admin, governance_token),
        );
        Ok(())
    }

    // ==========================================
    // Token Staking & Voting Power
    // ==========================================

    /// Stake STELLPOKER governance tokens to obtain voting power
    pub fn stake(env: Env, player: Address, amount: i128) -> Result<(), GovernanceError> {
        player.require_auth();
        if amount <= 0 {
            return Err(GovernanceError::InvalidAmount);
        }

        let gov_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceToken)
            .ok_or(GovernanceError::NotInitialized)?;

        // Transfer tokens to governance contract
        let token_client = token::Client::new(&env, &gov_token);
        token_client.transfer(&player, &env.current_contract_address(), &amount);

        // Update staked balances
        let current_staked: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::StakedBalance(player.clone()))
            .unwrap_or(0);
        let new_staked = current_staked.checked_add(amount).ok_or(GovernanceError::InvalidAmount)?;
        env.storage()
            .persistent()
            .set(&DataKey::StakedBalance(player.clone()), &new_staked);

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalStaked, &(total + amount));

        env.events().publish(
            (Symbol::new(&env, "tokens_staked"),),
            (player, amount, new_staked),
        );
        Ok(())
    }

    /// Unstake STELLPOKER tokens
    pub fn unstake(env: Env, player: Address, amount: i128) -> Result<(), GovernanceError> {
        player.require_auth();
        if amount <= 0 {
            return Err(GovernanceError::InvalidAmount);
        }

        let current_staked: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::StakedBalance(player.clone()))
            .unwrap_or(0);

        if current_staked < amount {
            return Err(GovernanceError::InsufficientVotingPower);
        }

        let new_staked = current_staked - amount;
        if new_staked == 0 {
            env.storage()
                .persistent()
                .remove(&DataKey::StakedBalance(player.clone()));
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::StakedBalance(player.clone()), &new_staked);
        }

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);
        let new_total = if total >= amount { total - amount } else { 0 };
        env.storage().instance().set(&DataKey::TotalStaked, &new_total);

        // Transfer tokens back to player
        let gov_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceToken)
            .unwrap();
        let token_client = token::Client::new(&env, &gov_token);
        token_client.transfer(&env.current_contract_address(), &player, &amount);

        env.events().publish(
            (Symbol::new(&env, "tokens_unstaked"),),
            (player, amount, new_staked),
        );
        Ok(())
    }

    /// Get current voting power for an account
    pub fn get_voting_power(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::StakedBalance(account))
            .unwrap_or(0)
    }

    /// Get total staked governance tokens
    pub fn get_total_staked(env: Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::TotalStaked)
            .unwrap_or(0)
    }

    // ==========================================
    // Proposal Lifecycle
    // ==========================================

    /// Submit a new governance proposal (rake, committee rewards, circuit upgrades, or table limits)
    pub fn propose(
        env: Env,
        proposer: Address,
        action: ProposalAction,
        title: String,
        description: String,
    ) -> Result<u32, GovernanceError> {
        proposer.require_auth();

        let config: GovernanceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(GovernanceError::NotInitialized)?;

        let voting_power = Self::get_voting_power(env.clone(), proposer.clone());
        if voting_power < config.proposal_threshold {
            return Err(GovernanceError::InsufficientVotingPower);
        }

        let proposal_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextProposalId)
            .unwrap_or(1);
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &(proposal_id + 1));

        let current_ledger = env.ledger().sequence();
        let start_ledger = current_ledger.saturating_add(config.voting_delay_ledgers);
        let end_ledger = start_ledger.saturating_add(config.voting_period_ledgers);

        let proposal = Proposal {
            id: proposal_id,
            proposer: proposer.clone(),
            action,
            title: title.clone(),
            description,
            start_ledger,
            end_ledger,
            execution_after_ledger: 0,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            status: ProposalStatus::Pending,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Proposal(proposal_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "proposal_created"),),
            (proposal_id, proposer, start_ledger, end_ledger),
        );
        Ok(proposal_id)
    }

    /// Cast a vote on an active proposal
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u32,
        vote: VoteType,
    ) -> Result<(), GovernanceError> {
        voter.require_auth();

        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        let current_ledger = env.ledger().sequence();
        if current_ledger < proposal.start_ledger || current_ledger > proposal.end_ledger {
            return Err(GovernanceError::ProposalNotActive);
        }

        let vote_key = DataKey::HasVoted(proposal_id, voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(GovernanceError::AlreadyVoted);
        }

        let voting_power = Self::get_voting_power(env.clone(), voter.clone());
        if voting_power <= 0 {
            return Err(GovernanceError::InsufficientVotingPower);
        }

        match vote {
            VoteType::For => {
                proposal.for_votes += voting_power;
            }
            VoteType::Against => {
                proposal.against_votes += voting_power;
            }
            VoteType::Abstain => {
                proposal.abstain_votes += voting_power;
            }
        }

        proposal.status = ProposalStatus::Active;
        env.storage().persistent().set(&vote_key, &true);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events().publish(
            (Symbol::new(&env, "vote_cast"),),
            (proposal_id, voter, voting_power),
        );
        Ok(())
    }

    /// Queue a successfully passed proposal for timelocked execution
    pub fn queue_proposal(env: Env, proposal_id: u32) -> Result<(), GovernanceError> {
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        let current_ledger = env.ledger().sequence();
        if current_ledger <= proposal.end_ledger {
            return Err(GovernanceError::VotingNotEnded);
        }

        let config: GovernanceConfig = env.storage().instance().get(&DataKey::Config).unwrap();
        let total_staked: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);

        let total_votes = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;

        // Check Quorum: total_votes * 10000 / total_staked >= quorum_bps
        if total_staked > 0 {
            let turnout_bps = (total_votes * BPS_SCALE) / total_staked;
            if turnout_bps < (config.quorum_bps as i128) {
                proposal.status = ProposalStatus::Defeated;
                env.storage()
                    .persistent()
                    .set(&DataKey::Proposal(proposal_id), &proposal);
                return Err(GovernanceError::QuorumNotMet);
            }
        }

        // Check Majority: for_votes * 10000 / (for_votes + against_votes) > approval_threshold_bps
        let decisive_votes = proposal.for_votes + proposal.against_votes;
        if decisive_votes == 0 {
            proposal.status = ProposalStatus::Defeated;
            env.storage()
                .persistent()
                .set(&DataKey::Proposal(proposal_id), &proposal);
            return Err(GovernanceError::ProposalNotPassed);
        }

        let approval_bps = (proposal.for_votes * BPS_SCALE) / decisive_votes;
        if approval_bps <= (config.approval_threshold_bps as i128) {
            proposal.status = ProposalStatus::Defeated;
            env.storage()
                .persistent()
                .set(&DataKey::Proposal(proposal_id), &proposal);
            return Err(GovernanceError::ProposalNotPassed);
        }

        proposal.status = ProposalStatus::Queued;
        proposal.execution_after_ledger = current_ledger.saturating_add(config.timelock_delay_ledgers);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_queued"),),
            (proposal_id, proposal.execution_after_ledger),
        );
        Ok(())
    }

    /// Execute a queued proposal once timelock delay has elapsed, updating active protocol parameters
    pub fn execute_proposal(env: Env, proposal_id: u32) -> Result<(), GovernanceError> {
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Queued {
            return Err(GovernanceError::InvalidStatus);
        }

        let current_ledger = env.ledger().sequence();
        if current_ledger < proposal.execution_after_ledger {
            return Err(GovernanceError::TimelockNotElapsed);
        }

        let mut params: ActiveProtocolParams = env
            .storage()
            .instance()
            .get(&DataKey::ActiveParams)
            .unwrap();

        // Apply parameter updates based on proposal action
        match &proposal.action {
            ProposalAction::SetRakePercentage(p) => {
                params.default_rake_bps = p.default_rake_bps;
                params.max_rake_bps = p.max_rake_bps;
            }
            ProposalAction::SetCommitteeRewards(p) => {
                params.committee_min_stake = p.min_stake;
                params.committee_reward_rate_bps = p.reward_rate_bps;
                params.committee_slash_penalty_bps = p.slash_penalty_bps;
            }
            ProposalAction::ProposeCircuitUpgrade(p) => {
                params.circuit_verifier = p.new_verifier.clone();
                params.circuit_hash = p.new_circuit_hash.clone();
            }
            ProposalAction::SetTableConfigLimits(p) => {
                params.table_min_buy_in_floor = p.min_buy_in_floor;
                params.table_max_players_limit = p.max_players_limit;
                params.table_min_timeout_seconds = p.min_action_timeout_seconds;
                params.table_max_rebuys_limit = p.max_rebuys_limit;
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::ActiveParams, &params);
        proposal.status = ProposalStatus::Executed;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_executed"),),
            proposal_id,
        );
        Ok(())
    }

    // ==========================================
    // Active Protocol Parameter Queries
    // ==========================================

    /// Get active protocol parameters
    pub fn get_active_params(env: Env) -> Result<ActiveProtocolParams, GovernanceError> {
        env.storage()
            .instance()
            .get::<DataKey, ActiveProtocolParams>(&DataKey::ActiveParams)
            .ok_or(GovernanceError::NotInitialized)
    }

    /// Get proposal by ID
    pub fn get_proposal(env: Env, proposal_id: u32) -> Result<Proposal, GovernanceError> {
        env.storage()
            .persistent()
            .get::<DataKey, Proposal>(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)
    }

    /// Check if account has voted on proposal
    pub fn has_voted(env: Env, proposal_id: u32, account: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::HasVoted(proposal_id, account))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Address, BytesN, Env, String};

    fn default_params(env: &Env, verifier: &Address) -> ActiveProtocolParams {
        ActiveProtocolParams {
            default_rake_bps: 250,
            max_rake_bps: 500,
            committee_min_stake: 10_000,
            committee_reward_rate_bps: 300,
            committee_slash_penalty_bps: 2000,
            circuit_verifier: verifier.clone(),
            circuit_hash: BytesN::from_array(env, &[0u8; 32]),
            table_min_buy_in_floor: 10,
            table_max_players_limit: 6,
            table_min_timeout_seconds: 15,
            table_max_rebuys_limit: 5,
        }
    }

    fn default_config() -> GovernanceConfig {
        GovernanceConfig {
            proposal_threshold: 1000,
            voting_delay_ledgers: 5,
            voting_period_ledgers: 20,
            timelock_delay_ledgers: 10,
            quorum_bps: 400,            // 4%
            approval_threshold_bps: 5000, // 50%
        }
    }

    #[test]
    fn test_stake_unstake_and_proposal_flow() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_sequence_number(100);

        let admin = Address::generate(&env);
        let voter1 = Address::generate(&env);
        let voter2 = Address::generate(&env);
        let verifier = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&voter1, &10_000);
        token_admin_client.mint(&voter2, &10_000);

        let contract_id = env.register(ProtocolGovernanceContract, ());
        let client = ProtocolGovernanceContractClient::new(&env, &contract_id);

        client.initialize(&admin, &token_id, &default_config(), &default_params(&env, &verifier));

        // Voters stake tokens
        client.stake(&voter1, &5000);
        client.stake(&voter2, &3000);

        assert_eq!(client.get_voting_power(&voter1), 5000);
        assert_eq!(client.get_voting_power(&voter2), 3000);
        assert_eq!(client.get_total_staked(), 8000);

        // Propose changing rake percentage from 250 bps to 200 bps
        let action = ProposalAction::SetRakePercentage(RakePayload {
            default_rake_bps: 200,
            max_rake_bps: 400,
        });
        let p_id = client.propose(
            &voter1,
            &action,
            &String::from_str(&env, "Lower Protocol Rake"),
            &String::from_str(&env, "Reduce rake to 2.0% to attract volume"),
        );
        assert_eq!(p_id, 1);

        // Advance ledger to start of voting (100 + 5 = 105)
        env.ledger().set_sequence_number(106);

        // Cast votes
        client.cast_vote(&voter1, &1, &VoteType::For);
        client.cast_vote(&voter2, &1, &VoteType::Against);

        let prop = client.get_proposal(&1);
        assert_eq!(prop.for_votes, 5000);
        assert_eq!(prop.against_votes, 3000);

        // Advance ledger past end of voting (105 + 20 = 125)
        env.ledger().set_sequence_number(126);

        // Queue proposal
        client.queue_proposal(&1);
        let queued = client.get_proposal(&1);
        assert_eq!(queued.status, ProposalStatus::Queued);
        assert_eq!(queued.execution_after_ledger, 126 + 10);

        // Advance past timelock
        env.ledger().set_sequence_number(137);

        // Execute proposal
        client.execute_proposal(&1);

        let updated_params = client.get_active_params();
        assert_eq!(updated_params.default_rake_bps, 200);
        assert_eq!(updated_params.max_rake_bps, 400);

        // Unstake part of tokens
        client.unstake(&voter1, &2000);
        assert_eq!(client.get_voting_power(&voter1), 3000);
        assert_eq!(client.get_total_staked(), 6000);
    }

    #[test]
    fn test_insufficient_voting_power_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let small_holder = Address::generate(&env);
        let verifier = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&small_holder, &500);

        let contract_id = env.register(ProtocolGovernanceContract, ());
        let client = ProtocolGovernanceContractClient::new(&env, &contract_id);
        client.initialize(&admin, &token_id, &default_config(), &default_params(&env, &verifier));

        client.stake(&small_holder, &500); // threshold is 1000
        let action = ProposalAction::SetRakePercentage(RakePayload {
            default_rake_bps: 150,
            max_rake_bps: 300,
        });
        assert!(client.try_propose(
            &small_holder,
            &action,
            &String::from_str(&env, "Rake Reduction"),
            &String::from_str(&env, "Desc")
        ).is_err());
    }

    #[test]
    fn test_committee_and_circuit_and_table_limits_proposals() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_sequence_number(200);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        let new_verifier = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&voter, &50_000);

        let contract_id = env.register(ProtocolGovernanceContract, ());
        let client = ProtocolGovernanceContractClient::new(&env, &contract_id);
        client.initialize(&admin, &token_id, &default_config(), &default_params(&env, &new_verifier));

        client.stake(&voter, &50_000);

        // 1. Propose Committee Rewards update
        let comm_action = ProposalAction::SetCommitteeRewards(CommitteePayload {
            min_stake: 20_000,
            reward_rate_bps: 450,
            slash_penalty_bps: 3000,
        });
        let p1 = client.propose(&voter, &comm_action, &String::from_str(&env, "Committee"), &String::from_str(&env, "D"));
        env.ledger().set_sequence_number(206);
        client.cast_vote(&voter, &p1, &VoteType::For);
        env.ledger().set_sequence_number(226);
        client.queue_proposal(&p1);
        env.ledger().set_sequence_number(237);
        client.execute_proposal(&p1);

        let params1 = client.get_active_params();
        assert_eq!(params1.committee_min_stake, 20_000);
        assert_eq!(params1.committee_reward_rate_bps, 450);

        // 2. Propose Table Limits update
        env.ledger().set_sequence_number(300);
        let table_action = ProposalAction::SetTableConfigLimits(TableLimitsPayload {
            min_buy_in_floor: 50,
            max_players_limit: 6,
            min_action_timeout_seconds: 20,
            max_rebuys_limit: 10,
        });
        let p2 = client.propose(&voter, &table_action, &String::from_str(&env, "Table Limits"), &String::from_str(&env, "D"));
        env.ledger().set_sequence_number(306);
        client.cast_vote(&voter, &p2, &VoteType::For);
        env.ledger().set_sequence_number(326);
        client.queue_proposal(&p2);
        env.ledger().set_sequence_number(337);
        client.execute_proposal(&p2);

        let params2 = client.get_active_params();
        assert_eq!(params2.table_min_buy_in_floor, 50);
        assert_eq!(params2.table_max_rebuys_limit, 10);
    }
}
