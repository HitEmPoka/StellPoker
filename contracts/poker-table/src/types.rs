use soroban_sdk::{contracterror, contracttype, Address, Bytes, BytesN, Env, Vec};

#[contracttype]
#[derive(Clone, Debug)]
pub struct TableConfig {
    pub token: Address, // Payment token (e.g., USDC)
    pub min_buy_in: i128,
    pub max_buy_in: i128,
    /// Blinds/ante structure for this table. A single-level schedule with
    /// `duration_seconds: 0` behaves as fixed blinds; multiple levels with
    /// nonzero `duration_seconds` produce an escalating (tournament-style)
    /// structure, optionally with an ante at any level.
    pub blinds_schedule: BlindsSchedule,
    /// Minimum seated players required to start a hand.
    pub min_players: u32,
    /// Maximum seated players allowed at the table. Capped at 6.
    pub max_players: u32,
    pub timeout_ledgers: u32, // Ledgers before timeout (~5 sec each)
    pub committee: Address,   // MPC committee address
    pub verifier: Address,    // ZK verifier contract address
    pub game_hub: Address,    // Game hub contract for start_game/end_game
    /// Rake taken from every pot, in basis points (100 = 1%). Capped at
    /// `MAX_RAKE_BPS` (500 = 5%); enforced on table creation.
    pub rake_bps: u32,
    /// How many times a seated player may top their stack up during one
    /// session at this table. `0` means unlimited. The counter resets when a
    /// player leaves and rejoins.
    pub max_rebuys: u32,
    /// Share of the total rake (in basis points) that is diverted into the
    /// bad-beat jackpot pool instead of going to the house. `0` disables the
    /// jackpot entirely.
    pub jackpot_rake_share_bps: u32,
    /// Minimum hand category required for a bad-beat qualifying hand
    /// (e.g. `7` = FourOfAKind).  Used alongside `min_bad_beat_rank` to
    /// compute the qualifying score threshold.
    pub min_bad_beat_category: u32,
    /// Minimum rank of the quad / trips / card required within the
    /// qualifying category (e.g. `12` = Ace).
    pub min_bad_beat_rank: u32,
    /// Per-street action time limits in seconds. Allows different time
    /// limits for each betting street (e.g. shorter for turbo tables).
    /// `None` falls back to the global `timeout_ledgers` converted to seconds.
    pub street_time_limit: Option<StreetTimeLimit>,
}

/// A single blinds/ante level in a table's schedule.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BlindLevel {
    pub small_blind: i128,
    pub big_blind: i128,
    /// Ante collected from every seated player at the start of each hand
    /// while this level is active. `0` means no ante.
    pub ante: i128,
    /// How long this level lasts once active, in seconds, before the
    /// schedule advances to the next level. Ignored on the final level
    /// (which lasts indefinitely once reached). `0` on a single-level
    /// schedule means the level never advances (fixed blinds).
    pub duration_seconds: u64,
    /// Optional break duration in seconds after this level expires, before
    /// the next level starts. During a break no hands are dealt; the
    /// coordinator must wait for the break to elapse before starting a new
    /// hand. `0` means no break. Break is only meaningful on non-final
    /// levels with a nonzero `duration_seconds`.
    pub break_seconds: u64,
}

/// Ordered blinds levels for a table. `levels[0]` is active from table
/// creation; the active level advances by wall-clock time as hands are
/// played (checked at the start of each new hand).
#[contracttype]
#[derive(Clone, Debug)]
pub struct BlindsSchedule {
    pub levels: Vec<BlindLevel>,
}

impl BlindsSchedule {
    /// A single-level, non-escalating schedule: fixed blinds, no ante.
    pub fn fixed(env: &Env, small_blind: i128, big_blind: i128) -> Self {
        let mut levels = Vec::new(env);
        levels.push_back(BlindLevel {
            small_blind,
            big_blind,
            ante: 0,
            duration_seconds: 0,
            break_seconds: 0,
        });
        BlindsSchedule { levels }
    }
}

/// A player waiting for a seat to open at a full table. `buy_in` has
/// already been transferred into contract escrow at queue-join time, so no
/// further authorization is needed from the player when they're auto-seated.
#[contracttype]
#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub player: Address,
    pub buy_in: i128,
}

/// A pending contract-wasm upgrade, committed to at `propose_upgrade` time
/// and only executable once `execute_after` has passed. `execute_upgrade`
/// always uses the hash stored here rather than one passed in again, so the
/// executed upgrade is guaranteed to match what was originally proposed.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UpgradeProposal {
    pub new_wasm_hash: BytesN<32>,
    pub execute_after: u64, // ledger timestamp (seconds)
}

/// Position where a straddle is posted.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum StraddlePosition {
    BigBlind,
    Utg,
}

/// Configuration for an optional straddle (2x or 3x big blind) that
/// can be posted before cards are dealt.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StraddleConfig {
    pub multiplier: u32, // 0 = disabled, 2 = 2x, 3 = 3x
    pub position: StraddlePosition,
}

/// Per-street action time limits in seconds. Allows different time limits
/// for each betting street (e.g. shorter for preflop in turbo tables).
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreetTimeLimit {
    /// Time limit for preflop actions, in seconds.
    pub preflop_seconds: u64,
    /// Time limit for flop actions, in seconds.
    pub flop_seconds: u64,
    /// Time limit for turn actions, in seconds.
    pub turn_seconds: u64,
    /// Time limit for river actions, in seconds.
    pub river_seconds: u64,
}

impl StreetTimeLimit {
    /// Standard time limits: 30s preflop, 60s for other streets.
    pub fn standard() -> Self {
        StreetTimeLimit {
            preflop_seconds: 30,
            flop_seconds: 60,
            turn_seconds: 60,
            river_seconds: 60,
        }
    }

    /// Turbo time limits: 15s preflop, 20s for other streets.
    pub fn turbo() -> Self {
        StreetTimeLimit {
            preflop_seconds: 15,
            flop_seconds: 20,
            turn_seconds: 20,
            river_seconds: 20,
        }
    }

    /// Get the time limit for a given game phase.
    pub fn for_phase(&self, phase: &GamePhase) -> u64 {
        match phase {
            GamePhase::Preflop => self.preflop_seconds,
            GamePhase::Flop => self.flop_seconds,
            GamePhase::Turn => self.turn_seconds,
            GamePhase::River => self.river_seconds,
            _ => self.preflop_seconds,
        }
    }
}

/// A cryptographic commitment to a player's action, stored on-chain for
/// dispute resolution. The coordinator submits commitments; in case of
/// dispute, anyone can request the coordinator to reveal the action data,
/// which is verified against the stored commitment.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ActionCommitment {
    /// The action the player took.
    pub action: Action,
    /// Amount for bet/raise actions.
    pub amount: i128,
    /// The seat of the player who took the action.
    pub seat: u32,
    /// The game phase when the action was taken.
    pub phase: GamePhase,
    /// Ledger timestamp when the commitment was stored.
    pub committed_at: u64,
    /// Whether this commitment has been revealed in a dispute.
    pub revealed: bool,
}

/// Aggregate hand type distribution statistics across all tables.
/// Used for proving randomness and fairness of the deal.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HandTypeDistribution {
    /// Count of each hand type: [HighCard, OnePair, TwoPair, ThreeOfAKind,
    /// Straight, Flush, FullHouse, FourOfAKind, StraightFlush, RoyalFlush]
    pub counts: Vec<u64>,
    /// Total number of hands observed.
    pub total_hands: u64,
    /// Total number of showdown hands (non-fold).
    pub total_showdowns: u64,
}

impl HandTypeDistribution {
    pub fn new(env: &Env) -> Self {
        let counts = Vec::from_array(env, [0u64; 10]);
        HandTypeDistribution {
            counts,
            total_hands: 0,
            total_showdowns: 0,
        }
    }
}

/// Bookkeeping for per-table action commitments used in dispute resolution.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ActionCommitmentMeta {
    /// Next action index for the current hand.
    pub next_index: u32,
    /// Number of commitments stored for the current hand.
    pub stored: u32,
}

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PokerTableError {
    TableNotFound = 1,
    TableNotAcceptingPlayers = 2,
    TableFull = 3,
    InvalidBuyIn = 4,
    AlreadySeated = 5,
    PlayerNotAtTable = 6,
    CannotLeaveDuringActiveHand = 7,
    HandAlreadyInProgress = 8,
    NotEnoughPlayers = 9,
    InvalidPlayerIndex = 10,
    NotYourTurn = 11,
    PlayerAlreadyFolded = 12,
    PlayerAlreadyAllIn = 13,
    MustCallOrFold = 14,
    NothingToCall = 15,
    CannotBetWhenOutstandingBet = 16,
    BetTooSmall = 17,
    RaiseTooSmall = 18,
    NotEnoughChips = 19,
    NotInBettingPhase = 20,
    NotInDealingPhase = 21,
    NotInRevealPhase = 22,
    NotInShowdownPhase = 23,
    WrongCommitmentCount = 24,
    WrongCardCount = 25,
    NotAuthorizedCommittee = 26,
    DealProofVerificationFailed = 27,
    RevealProofVerificationFailed = 28,
    ShowdownProofVerificationFailed = 29,
    BoardNotComplete = 30,
    InvalidHoleCards = 31,
    TimeoutNotReached = 32,
    TimeoutNotApplicable = 33,
    HoleCardMismatch = 34,
    WinnerNotEligibleForPot = 35,
    RakeBpsExceedsMax = 36,
    InvalidPlayerCount = 37,
    CannotChangeMinPlayersMidHand = 38,
    ContractPaused = 39,
    ForceFoldNotAvailable = 40,
    TargetNotActive = 41,
    CannotRebuyDuringActiveHand = 42,
    RebuyLimitReached = 43,
    InvalidRebuyAmount = 44,
    NotInRitPhase = 45,
    RitAlreadyDecided = 46,
    NotHeadsUpAllIn = 47,
    RunItTwiceNotEnabled = 48,
    RitAlreadyActive = 49,
    BoardAlreadyRevealedForRun = 50,
    JackpotNotConfigured = 51,
    BadBeatHandDataInvalid = 52,
    StaleActionSequence = 53,
    EmptyBlindsSchedule = 54,
    InvalidBlindLevel = 55,
    AlreadyQueued = 56,
    NotQueued = 57,
    QueueFull = 58,
    NoUpgradeProposal = 59,
    UpgradeDelayNotElapsed = 60,
    UpgradeDelayTooShort = 61,
    InvalidVarianceConfig = 62,
    TableClosureNotProposed = 63,
    TableClosureNoticeActive = 64,
    TableClosureNotReady = 65,
    TableClosureInProgress = 66,
    InvalidStraddleConfig = 62,
    EmergencyWithdrawalNotApplicable = 63,
    EmergencyTimelockActive = 64,
    AlreadyApprovedEmergencyWithdrawal = 65,
    ActionAlreadyCommitted = 66,
    ActionCommitmentNotFound = 67,
    InvalidActionReveal = 68,
    InvalidHandType = 69,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub address: Address,
    pub stack: i128,
    pub bet_this_round: i128,
    /// Total chips this player has committed to the pot across every betting
    /// round of the current hand. Used to compute multi-way side pots, since a
    /// player can only win the chips they themselves have contributed to.
    pub committed: i128,
    pub folded: bool,
    pub all_in: bool,
    pub sitting_out: bool,
    pub seat_index: u32,
    /// Every chip this player has deposited at the table this session — the
    /// initial buy-in plus every rebuy. Used to compute session profit and to
    /// audit chip conservation independently of the current stack.
    pub total_buy_in: i128,
    /// Rebuys used this session, checked against `TableConfig::max_rebuys`.
    pub rebuy_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum GamePhase {
    Waiting,      // Waiting for players
    Dealing,      // Committee is dealing
    Preflop,      // Betting round: preflop
    DealingFlop,  // Committee revealing flop
    Flop,         // Betting round: flop
    DealingTurn,  // Committee revealing turn
    Turn,         // Betting round: turn
    DealingRiver, // Committee revealing river
    River,        // Betting round: river
    Showdown,     // Revealing hands and determining winner
    Settlement,   // Pot distributed, ready for next hand
    Dispute,      // Something went wrong; funds frozen
    // Run-It-Twice phases
    AwaitingRunItTwice, // Waiting for all-in players to decide on RIT
    ShowdownRun1,     // First run's showdown
    ShowdownRun2,     // Second run's showdown
    RitSettlement,    // Pot split between two runs
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum Action {
    Fold,
    Check,
    Call,
    Bet(i128),
    Raise(i128),
    AllIn,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SidePot {
    pub amount: i128,
    pub eligible_players: Vec<u32>, // seat indices
}

/// State tracking for Run-It-Twice when two players are all-in heads-up.
/// RIT deals the remaining board twice and splits the pot based on wins.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RitState {
    pub active: bool,
    /// Seat indices of the two all-in players who opted in
    pub player1_seat: u32,
    pub player2_seat: u32,
    pub player1_opted_in: bool,
    pub player2_opted_in: bool,
    /// Number of board cards already revealed before RIT was activated
    /// (0 = preflop, 3 = flop, 4 = turn)
    pub shared_board_count: u32,
    /// Which run we're currently dealing (0 = not started, 1 or 2)
    pub current_run: u32,
    /// Deck indices for Run 1's full 5-card board (shared + remaining)
    pub run1_board_indices: Vec<u32>,
    /// Deck indices for Run 2's full 5-card board (shared + run2 remaining)
    pub run2_board_indices: Vec<u32>,
    /// Winner seat for Run 1
    pub run1_winner: u32,
    /// Winner seat for Run 2
    pub run2_winner: u32,
}

/// The kind of a betting action, without its amount. Stored in hand history
/// where the chips moved are recorded separately in `ActionRecord::amount`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ActionKind {
    Fold,
    Check,
    Call,
    Bet,
    Raise,
    AllIn,
}

/// One entry of a hand's action summary.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ActionRecord {
    pub seat: u32,
    /// Betting round the action was taken in.
    pub phase: GamePhase,
    pub kind: ActionKind,
    /// Chips this action added to the pot (0 for fold/check).
    pub amount: i128,
}

/// Chips credited to a single seat when a hand settled.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payout {
    pub seat: u32,
    pub address: Address,
    pub amount: i128,
}

/// An immutable record of one completed hand, retained in the table's circular
/// hand-history buffer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HandRecord {
    pub hand_number: u32,
    /// Seat-ordered addresses of the players dealt into the hand.
    pub players: Vec<Address>,
    /// Community cards as they stood when the hand ended (may be shorter than
    /// five if everyone folded before the river).
    pub board: Vec<u32>,
    /// Betting actions in the order they were taken, truncated at
    /// `history::MAX_ACTIONS_PER_HAND`.
    pub actions: Vec<ActionRecord>,
    /// How the pot was split, one entry per paid seat.
    pub payouts: Vec<Payout>,
    /// Pot size before rake was deducted.
    pub total_pot: i128,
    pub rake: i128,
    /// True when the hand ended by showdown, false when everyone else folded.
    pub showdown: bool,
    pub settled_ledger: u32,
}

/// Bookkeeping for a table's circular hand-history buffer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HandHistoryMeta {
    /// Slot the next archived hand will be written to.
    pub next_slot: u32,
    /// Records currently stored, saturating at the buffer capacity.
    pub stored: u32,
    /// Hands archived over the table's lifetime, including evicted ones.
    pub total_archived: u32,
}

/// Cumulative winner distribution used to detect unusually concentrated table
/// outcomes. Counts are indexed by seat.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VarianceStats {
    pub hands: u32,
    pub winner_counts: Vec<u32>,
    pub variance_bps: u32,
}

/// Controls when concentrated outcomes receive extra jackpot funding.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VarianceConfig {
    pub threshold_bps: u32,
    pub extra_jackpot_share_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TableClosureProposal {
    pub execute_after: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TableState {
    pub id: u32,
    pub admin: Address,
    pub config: TableConfig,
    pub phase: GamePhase,
    pub players: Vec<PlayerState>,
    pub dealer_seat: u32,
    pub current_turn: u32,
    pub pot: i128,
    pub side_pots: Vec<SidePot>,
    pub deck_root: BytesN<32>,
    pub hand_commitments: Vec<BytesN<32>>,
    pub board_cards: Vec<u32>,   // Revealed community cards
    pub dealt_indices: Vec<u32>, // Deck indices already dealt
    pub hand_number: u32,
    pub last_action_ledger: u32, // For timeout calculation
    pub committee: Address,
    pub session_id: u32, // Game hub session ID for current hand
    /// Accumulated rake collected from settled hands, withdrawable by `admin`.
    pub rake_balance: i128,
    /// Accumulated bad-beat jackpot pool, fed by a share of each hand's rake.
    /// Paid out when a qualifying bad beat occurs at showdown.
    pub jackpot_balance: i128,
    /// Ledger sequence by which the current player must act. Any other seated
    /// player may call `force_fold` after this deadline is reached.
    pub action_deadline: u32,
    /// Betting actions taken so far in the current hand. Cleared when a hand
    /// starts and archived into the hand-history buffer when it settles.
    pub hand_actions: Vec<ActionRecord>,
    /// Run-It-Twice state when two players are all-in heads-up.
    pub rit_state: Option<RitState>,
    /// Size of the last bet or raise in the current betting round.
    /// The next raise must be at least this large (standard poker minimum-raise
    /// rule). Cleared to `big_blind` when a new betting round begins.
    pub last_raise_size: i128,
    /// Index into `config.blinds_schedule.levels` of the currently active
    /// blinds level.
    pub current_blind_level: u32,
    /// Ledger timestamp (seconds) at which the current blind level began.
    pub level_started_at: u64,
    /// If the current level has a break, this is the timestamp (seconds) at
    /// which the break ends and the next level becomes active. `0` means no
    /// break is in progress.
    pub break_ends_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Table(u32),
    Paused(u32), // per-table pause flag
    /// One archived hand: (table_id, circular buffer slot).
    HandRecord(u32, u32),
    /// Circular buffer bookkeeping for a table's hand history.
    HandHistoryMeta(u32),
    /// Tables a wallet is currently seated at, for multi-table clients.
    PlayerTables(Address),
    /// Per-player per-table monotonically increasing action sequence counter.
    /// Used to reject stale or replayed betting actions.
    PlayerActionCounter(u32, Address),
    Queue(u32),  // waiting-list queue for a full table
    UpgradeProposal(u32),
    /// Per-table outcome distribution and variance state.
    VarianceStats(u32),
    /// Per-table variance-triggered jackpot funding configuration.
    VarianceConfig(u32),
    /// Pending forced closure notice for a table.
    TableClosure(u32),
    /// Per-table straddle configuration.
    StraddleConfig(u32),
    /// Seat index of the player who posted a straddle for the current hand.
    ActiveStraddleSeat(u32),
    /// Addresses of players who approved emergency withdrawal for a table.
    EmergencyApprovals(u32),
    /// Action commitment for dispute resolution: (table_id, hand_number, action_index).
    ActionCommitment(u32, u32, u32),
    /// Bookkeeping for action commitments per table.
    ActionCommitmentMeta(u32),
    /// Aggregate hand type distribution statistics (global).
    HandTypeDistribution,
}
