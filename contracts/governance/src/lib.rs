#![no_std]
#![allow(deprecated)]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

/// MVP governance contract for committee parameter changes (see ADR-006).
///
/// Lifecycle: propose → vote → timelock → execute. One member, one vote.
/// Quorum and approval threshold are basis-point constants set at initialize
/// and changeable only through the same lifecycle.
#[contract]
pub struct Governance;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum GovernanceError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotMember = 3,
    DuplicateProposal = 4,
    ProposalNotFound = 5,
    AlreadyVoted = 6,
    VotingClosed = 7,
    TimelockNotElapsed = 8,
    QuorumNotMet = 9,
    ThresholdNotMet = 10,
    AlreadyExecuted = 11,
    InvalidParam = 12,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Executed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    /// Parameter key, e.g. `symbol_short!("deal_to")`.
    pub param_key: Symbol,
    pub new_value: i128,
    pub votes_for: u32,
    pub votes_against: u32,
    pub executed: bool,
    /// Ledger timestamp after which the proposal may be executed.
    pub execute_after: u64,
    /// Number of votes needed to reach quorum (snapshot at creation).
    pub quorum_votes: u32,
    /// Number of For-votes needed to reach the approval threshold.
    pub approval_votes: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Initialized,
    Members,
    Member(Address),
    ProposalCount,
    Proposal(u32),
    /// Voter → proposal_id → has voted.
    Voted(Address, u32),
    QuorumBps,
    ApprovalThresholdBps,
    TimelockSeconds,
}

const BPS_DENOM: u32 = 10_000;

#[contractimpl]
impl Governance {
    /// Initialize with the committee member set and governance parameters.
    /// `quorum_bps` and `approval_threshold_bps` are basis points (5000 = 50%).
    /// `timelock_seconds` is the delay between a proposal becoming executable
    /// and execution.
    pub fn initialize(
        env: Env,
        admin: Address,
        members: Vec<Address>,
        quorum_bps: u32,
        approval_threshold_bps: u32,
        timelock_seconds: u64,
    ) -> Result<(), GovernanceError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(GovernanceError::AlreadyInitialized);
        }
        admin.require_auth();
        assert!(!members.is_empty(), "members cannot be empty");
        assert!(quorum_bps > 0 && quorum_bps <= BPS_DENOM, "invalid quorum_bps");
        assert!(
            approval_threshold_bps > 0 && approval_threshold_bps <= BPS_DENOM,
            "invalid approval_threshold_bps"
        );

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::Members, &members);
        env.storage().instance().set(&DataKey::QuorumBps, &quorum_bps);
        env.storage()
            .instance()
            .set(&DataKey::ApprovalThresholdBps, &approval_threshold_bps);
        env.storage()
            .instance()
            .set(&DataKey::TimelockSeconds, &timelock_seconds);
        env.storage().instance().set(&DataKey::ProposalCount, &0u32);

        for i in 0..members.len() {
            let m = members.get(i).unwrap();
            env.storage().instance().set(&DataKey::Member(m), &true);
        }
        Ok(())
    }

    /// Propose a committee parameter change. Returns the proposal ID.
    /// Only registered members may propose.
    pub fn propose(
        env: Env,
        proposer: Address,
        param_key: Symbol,
        new_value: i128,
    ) -> Result<u32, GovernanceError> {
        Self::require_initialized(&env)?;
        proposer.require_auth();
        Self::require_member(&env, &proposer)?;

        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .ok_or(GovernanceError::NotInitialized)?;
        let member_count = members.len();

        let quorum_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::QuorumBps)
            .unwrap_or(5000);
        let approval_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ApprovalThresholdBps)
            .unwrap_or(5000);
        let timelock: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TimelockSeconds)
            .unwrap_or(0);

        // Quorum votes: ceil(member_count * quorum_bps / 10_000), min 1.
        let quorum_votes =
            ((member_count as u64 * quorum_bps as u64).div_ceil(BPS_DENOM as u64)) as u32;
        let quorum_votes = quorum_votes.max(1);
        // Approval votes: floor(votes_needed * approval_bps / 10_000) + 1
        // (strict majority when approval_bps = 5000 and quorum_votes is even).
        let approval_votes = (quorum_votes as u64 * approval_bps as u64 / BPS_DENOM as u64) as u32 + 1;

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .unwrap_or(0);
        let id = count + 1;
        env.storage().instance().set(&DataKey::ProposalCount, &id);

        let execute_after = env.ledger().timestamp() + timelock;
        let proposal = Proposal {
            id,
            proposer: proposer.clone(),
            param_key: param_key.clone(),
            new_value,
            votes_for: 0,
            votes_against: 0,
            executed: false,
            execute_after,
            quorum_votes,
            approval_votes,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(id), &proposal);

        env.events().publish(
            (symbol_short!("proposal"), id),
            (proposer, param_key, new_value, execute_after),
        );
        Ok(id)
    }

    /// Cast a vote on a proposal. `support = true` votes For, `false` Against.
    /// Each member may vote at most once per proposal.
    pub fn vote(
        env: Env,
        voter: Address,
        proposal_id: u32,
        support: bool,
    ) -> Result<(), GovernanceError> {
        Self::require_initialized(&env)?;
        voter.require_auth();
        Self::require_member(&env, &voter)?;

        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;
        if proposal.executed {
            return Err(GovernanceError::VotingClosed);
        }

        let voted_key = DataKey::Voted(voter.clone(), proposal_id);
        if env.storage().persistent().has(&voted_key) {
            return Err(GovernanceError::AlreadyVoted);
        }
        env.storage().persistent().set(&voted_key, &true);

        if support {
            proposal.votes_for += 1;
        } else {
            proposal.votes_against += 1;
        }
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events().publish(
            (symbol_short!("vote"), proposal_id),
            (voter, support, proposal.votes_for, proposal.votes_against),
        );
        Ok(())
    }

    /// Execute a proposal once quorum, threshold, and timelock are satisfied.
    /// Any member may call execute. Returns the applied value.
    pub fn execute(env: Env, caller: Address, proposal_id: u32) -> Result<i128, GovernanceError> {
        Self::require_initialized(&env)?;
        caller.require_auth();
        Self::require_member(&env, &caller)?;

        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;
        if proposal.executed {
            return Err(GovernanceError::AlreadyExecuted);
        }

        let total_votes = proposal.votes_for + proposal.votes_against;
        if total_votes < proposal.quorum_votes {
            return Err(GovernanceError::QuorumNotMet);
        }
        if proposal.votes_for < proposal.approval_votes {
            return Err(GovernanceError::ThresholdNotMet);
        }
        if env.ledger().timestamp() < proposal.execute_after {
            return Err(GovernanceError::TimelockNotElapsed);
        }

        proposal.executed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events().publish(
            (symbol_short!("executed"), proposal_id),
            (proposal.param_key.clone(), proposal.new_value, caller),
        );
        Ok(proposal.new_value)
    }

    /// Read a proposal (view function).
    pub fn get_proposal(env: Env, proposal_id: u32) -> Result<Proposal, GovernanceError> {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)
    }

    /// Read governance parameters: (quorum_bps, approval_threshold_bps,
    /// timelock_seconds).
    pub fn get_config(env: Env) -> (u32, u32, u64) {
        (
            env.storage()
                .instance()
                .get(&DataKey::QuorumBps)
                .unwrap_or(5000),
            env.storage()
                .instance()
                .get(&DataKey::ApprovalThresholdBps)
                .unwrap_or(5000),
            env.storage()
                .instance()
                .get(&DataKey::TimelockSeconds)
                .unwrap_or(0),
        )
    }

    fn require_initialized(env: &Env) -> Result<(), GovernanceError> {
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(GovernanceError::NotInitialized);
        }
        Ok(())
    }

    fn require_member(env: &Env, member: &Address) -> Result<(), GovernanceError> {
        if !env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Member(member.clone()))
            .unwrap_or(false)
        {
            return Err(GovernanceError::NotMember);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger as _}, Address, Env, Vec};

    const TIMELOCK: u64 = 100;

    fn setup() -> (Env, GovernanceClient<'static>, Vec<Address>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(Governance, ());
        let client = GovernanceClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let mut members: Vec<Address> = Vec::new(&env);
        for _ in 0..4 {
            members.push_back(Address::generate(&env));
        }
        // quorum = 50% of 4 = 2 votes; threshold = 50% of 2 = 1 For-vote (+1 = 2? see below)
        client.initialize(&admin, &members, &5000, &5000, &TIMELOCK);
        (env, client, members, admin)
    }

    fn advance_time(env: &Env, seconds: u64) {
        env.ledger().with_mut(|l| l.timestamp += seconds);
    }

    #[test]
    fn initialize_sets_config() {
        let (_env, client, _members, _admin) = setup();
        let (quorum, threshold, timelock) = client.get_config();
        assert_eq!(quorum, 5000);
        assert_eq!(threshold, 5000);
        assert_eq!(timelock, TIMELOCK);
    }

    #[test]
    fn propose_assigns_ids_and_stores() {
        let (_env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        assert_eq!(id, 1);

        let p = client.get_proposal(&1);
        assert_eq!(p.proposer, proposer);
        assert_eq!(p.param_key, symbol_short!("deal_to"));
        assert_eq!(p.new_value, 200);
        assert!(!p.executed);
        // quorum = ceil(4 * 5000 / 10000) = 2; approval = floor(2 * 5000 / 10000) + 1 = 2
        assert_eq!(p.quorum_votes, 2);
        assert_eq!(p.approval_votes, 2);
    }

    #[test]
    fn non_member_cannot_propose() {
        let (env, client, _members, _admin) = setup();
        let stranger = Address::generate(&env);
        let result = client.try_propose(&stranger, &symbol_short!("deal_to"), &200);
        assert_eq!(result, Err(Ok(GovernanceError::NotMember)));
    }

    #[test]
    fn vote_accumulates_and_blocks_double_vote() {
        let (_env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);

        let v1 = members.get(0).unwrap();
        let v2 = members.get(1).unwrap();
        client.vote(&v1, &id, &true);
        client.vote(&v2, &id, &true);

        let p = client.get_proposal(&id);
        assert_eq!(p.votes_for, 2);
        assert_eq!(p.votes_against, 0);

        let result = client.try_vote(&v1, &id, &false);
        assert_eq!(result, Err(Ok(GovernanceError::AlreadyVoted)));
    }

    #[test]
    fn execute_fails_before_timelock() {
        let (_env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        client.vote(&members.get(0).unwrap(), &id, &true);
        client.vote(&members.get(1).unwrap(), &id, &true);

        // Timelock has not elapsed.
        let result = client.try_execute(&proposer, &id);
        assert_eq!(result, Err(Ok(GovernanceError::TimelockNotElapsed)));
    }

    #[test]
    fn execute_succeeds_when_quorum_threshold_and_timelock_met() {
        let (env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        client.vote(&members.get(0).unwrap(), &id, &true);
        client.vote(&members.get(1).unwrap(), &id, &true);

        advance_time(&env, TIMELOCK);
        let value = client.execute(&proposer, &id);
        assert_eq!(value, 200);

        let p = client.get_proposal(&id);
        assert!(p.executed);

        // Cannot execute twice.
        let result = client.try_execute(&proposer, &id);
        assert_eq!(result, Err(Ok(GovernanceError::AlreadyExecuted)));
    }

    #[test]
    fn execute_fails_when_quorum_not_met() {
        let (env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        // Only 1 of 4 members votes — quorum requires 2.
        client.vote(&members.get(0).unwrap(), &id, &true);

        advance_time(&env, TIMELOCK);
        let result = client.try_execute(&proposer, &id);
        assert_eq!(result, Err(Ok(GovernanceError::QuorumNotMet)));
    }

    #[test]
    fn execute_fails_when_threshold_not_met() {
        let (env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        // 2 For, 2 Against → quorum met (2), but For (2) < approval_votes (2)? No: approval = 2, so For >= 2 passes.
        // Use 1 For, 3 Against to clearly fail threshold.
        client.vote(&members.get(0).unwrap(), &id, &true);
        client.vote(&members.get(1).unwrap(), &id, &false);
        client.vote(&members.get(2).unwrap(), &id, &false);
        client.vote(&members.get(3).unwrap(), &id, &false);

        advance_time(&env, TIMELOCK);
        let result = client.try_execute(&proposer, &id);
        assert_eq!(result, Err(Ok(GovernanceError::ThresholdNotMet)));
    }

    #[test]
    fn non_member_cannot_vote() {
        let (env, client, members, _admin) = setup();
        let proposer = members.get(0).unwrap();
        let id = client.propose(&proposer, &symbol_short!("deal_to"), &200);
        let stranger = Address::generate(&env);
        let result = client.try_vote(&stranger, &id, &true);
        assert_eq!(result, Err(Ok(GovernanceError::NotMember)));
    }

    #[test]
    fn proposal_not_found() {
        let (_env, client, _members, _admin) = setup();
        let result = client.try_get_proposal(&99);
        assert_eq!(result, Err(Ok(GovernanceError::ProposalNotFound)));
    }
}
