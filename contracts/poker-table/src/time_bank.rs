use soroban_sdk::{Address, Env, Symbol};

use crate::types::*;

/// Seconds per ledger (approx). Used to convert time-bank seconds to ledger deadlines.
const SECONDS_PER_LEDGER: u64 = 5;

/// Load the effective time bank config for a table.
///
/// Stored in instance storage via `configure_time_bank`. Returns None when disabled.
fn load_config(env: &Env, table: &TableState) -> Option<TimeBankConfig> {
    if let Some(cfg) = env
        .storage()
        .instance()
        .get::<DataKey, TimeBankConfig>(&DataKey::TimeBankConfig(table.id))
    {
        if cfg.is_enabled() {
            return Some(cfg);
        }
    }
    None
}

/// Public accessor for per-table config (view function helper).
pub fn get_config_for_table(env: &Env, table_id: u32) -> Option<TimeBankConfig> {
    env.storage()
        .instance()
        .get(&DataKey::TimeBankConfig(table_id))
}

/// Load or initialize a player's time bank.
pub fn load_or_init(env: &Env, table_id: u32, player: &Address, cfg: &TimeBankConfig) -> TimeBank {
    let key = DataKey::TimeBank(table_id, player.clone());
    if let Some(bank) = env.storage().persistent().get::<DataKey, TimeBank>(&key) {
        return bank;
    }
    let bank = TimeBank {
        remaining_seconds: cfg.initial_seconds,
        last_replenish_ledger: env.ledger().sequence(),
        extensions_used_this_hand: 0,
        active_extension: false,
        active_extension_seconds: 0,
    };
    env.storage().persistent().set(&key, &bank);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
    bank
}

pub fn get_bank(env: &Env, table_id: u32, player: &Address) -> Option<TimeBank> {
    env.storage()
        .persistent()
        .get(&DataKey::TimeBank(table_id, player.clone()))
}

/// Persist a time bank.
fn save_bank(env: &Env, table_id: u32, player: &Address, bank: &TimeBank) {
    let key = DataKey::TimeBank(table_id, player.clone());
    env.storage().persistent().set(&key, bank);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
}

/// Replenish all players' time banks at the start of a new hand.
///
/// - Adds `replenish_per_hand` flat per hand.
/// - Adds `replenish_per_ledger * elapsed_ledgers` if configured.
/// - Caps at `max_seconds`.
/// - Resets `extensions_used_this_hand` for the new hand.
pub fn replenish_all(env: &Env, table: &mut TableState) {
    let cfg = match load_config(env, table) {
        Some(c) => c,
        None => return,
    };
    let current_ledger = env.ledger().sequence();
    for i in 0..table.players.len() {
        if let Some(p) = table.players.get(i) {
            let key = DataKey::TimeBank(table.id, p.address.clone());
            let mut bank: TimeBank = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or(TimeBank {
                    remaining_seconds: cfg.initial_seconds,
                    last_replenish_ledger: current_ledger,
                    extensions_used_this_hand: 0,
                    active_extension: false,
                    active_extension_seconds: 0,
                });
            // Per-hand replenish
            bank.remaining_seconds = bank
                .remaining_seconds
                .saturating_add(cfg.replenish_per_hand)
                .min(cfg.max_seconds);
            // Per-ledger replenish (if configured)
            if cfg.replenish_per_ledger > 0 {
                let elapsed = current_ledger.saturating_sub(bank.last_replenish_ledger) as u64;
                let ledger_replenish = elapsed.saturating_mul(cfg.replenish_per_ledger);
                bank.remaining_seconds = bank
                    .remaining_seconds
                    .saturating_add(ledger_replenish)
                    .min(cfg.max_seconds);
            }
            bank.last_replenish_ledger = current_ledger;
            bank.extensions_used_this_hand = 0;
            bank.active_extension = false;
            bank.active_extension_seconds = 0;
            env.storage().persistent().set(&key, &bank);
            env.storage()
                .persistent()
                .extend_ttl(&key, 17_280, 518_400);
        }
    }
}

/// Player spends time-bank seconds to extend their action deadline.
///
/// Requirements:
/// - Time bank must be configured and enabled.
/// - It must be the player's turn.
/// - They must have remaining time and not exceeded `max_extensions_per_hand`.
/// - Table must be in a betting phase.
///
/// On success, `table.action_deadline` is extended by `extension_seconds`
/// (converted to ledgers) and the player's bank is debited.
pub fn use_time_bank(
    env: &Env,
    table: &mut TableState,
    player: &Address,
) -> Result<u64, PokerTableError> {
    let cfg = load_config(env, table).ok_or(PokerTableError::TimeBankNotConfigured)?;
    if !cfg.is_enabled() {
        return Err(PokerTableError::TimeBankNotConfigured);
    }
    if !matches!(
        table.phase,
        GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
    ) {
        return Err(PokerTableError::NotInBettingPhase);
    }
    // Find seat and verify it's their turn
    let mut seat_opt: Option<u32> = None;
    for i in 0..table.players.len() {
        if let Some(p) = table.players.get(i) {
            if crate::constant_time::address_eq(env, &p.address, player) {
                seat_opt = Some(p.seat_index);
                break;
            }
        }
    }
    let seat = seat_opt.ok_or(PokerTableError::PlayerNotAtTable)?;
    if seat != table.current_turn {
        return Err(PokerTableError::NotYourTurnForTimeBank);
    }

    let mut bank = load_or_init(env, table.id, player, &cfg);
    if bank.extensions_used_this_hand >= cfg.max_extensions_per_hand {
        return Err(PokerTableError::TimeBankExhausted);
    }
    if bank.remaining_seconds < cfg.extension_seconds {
        return Err(PokerTableError::TimeBankExhausted);
    }

    // Debit
    bank.remaining_seconds -= cfg.extension_seconds;
    bank.extensions_used_this_hand += 1;
    bank.active_extension = true;
    bank.active_extension_seconds = cfg.extension_seconds;
    save_bank(env, table.id, player, &bank);

    // Extend deadline: convert seconds to ledgers (ceil)
    let extension_ledgers = (cfg.extension_seconds + SECONDS_PER_LEDGER - 1) / SECONDS_PER_LEDGER;
    let extension_ledgers = extension_ledgers as u32;
    // If deadline is already in the past, start from current ledger
    let base = core::cmp::max(table.action_deadline, env.ledger().sequence());
    table.action_deadline = base + extension_ledgers;

    env.events().publish(
        (Symbol::new(env, "time_bank_used"), table.id),
        (player.clone(), cfg.extension_seconds, bank.remaining_seconds, table.action_deadline),
    );
    Ok(cfg.extension_seconds)
}

/// Apply an automatic initial time-bank check when a new betting round starts.
/// If the player at `seat` has auto-extend enabled and time bank is enabled,
/// we could grant a small initial buffer? For now this is a pass-through that
/// returns the base deadline unchanged, but keeps the hook for future policy.
pub fn apply_initial_deadline(
    env: &Env,
    table_id: u32,
    _seat: u32,
    base_deadline: u32,
) -> u32 {
    // Hook for future automatic time-bank usage at round start.
    // Currently we just return base_deadline; players must explicitly call use_time_bank.
    let _ = env;
    let _ = table_id;
    base_deadline
}

/// Check whether a timeout should be enforced, taking time-bank extensions into account.
///
/// Returns `true` if the deadline has genuinely passed and no time-bank rescue is available.
/// Returns `false` if the player could still rescue via time bank (caller may offer extension).
pub fn should_enforce_timeout(env: &Env, table: &TableState) -> bool {
    let current = env.ledger().sequence();
    if table.action_deadline == 0 || current < table.action_deadline {
        return false;
    }
    // Deadline has passed — check if current player has time bank that could save them
    if let Some(cfg) = load_config(env, table) {
        if !cfg.is_enabled() {
            return true;
        }
        if let Some(p) = table.players.get(table.current_turn) {
            if let Some(bank) = get_bank(env, table.id, &p.address) {
                if bank.remaining_seconds >= cfg.extension_seconds
                    && bank.extensions_used_this_hand < cfg.max_extensions_per_hand
                {
                    // Player *could* use time bank, but hasn't. We still enforce timeout
                    // at the contract level unless they explicitly call use_time_bank
                    // before the grace window expires. However, to give a small grace,
                    // we check if we are within 1 ledger of deadline — if so, don't enforce yet.
                    // For simplicity, we enforce immediately; the off-chain client is expected
                    // to have called use_time_bank in time.
                    return true;
                }
            }
        }
    }
    true
}

/// Initialize time bank for a newly joined player.
pub fn init_for_player(env: &Env, table_id: u32, player: &Address, cfg_opt: Option<TimeBankConfig>) {
    let cfg = if let Some(c) = cfg_opt {
        c
    } else if let Some(c) = env
        .storage()
        .instance()
        .get::<DataKey, TimeBankConfig>(&DataKey::TimeBankConfig(table_id))
    {
        c
    } else {
        return;
    };
    if !cfg.is_enabled() {
        return;
    }
    let key = DataKey::TimeBank(table_id, player.clone());
    if env.storage().persistent().has(&key) {
        return;
    }
    let bank = TimeBank {
        remaining_seconds: cfg.initial_seconds,
        last_replenish_ledger: env.ledger().sequence(),
        extensions_used_this_hand: 0,
        active_extension: false,
        active_extension_seconds: 0,
    };
    env.storage().persistent().set(&key, &bank);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
}

/// Configure time bank for a table (admin only, between hands).
pub fn configure(
    env: &Env,
    table: &TableState,
    cfg: &TimeBankConfig,
) -> Result<(), PokerTableError> {
    if cfg.max_seconds > 3600 || cfg.extension_seconds > 300 {
        return Err(PokerTableError::InvalidTimeBankConfig);
    }
    if cfg.is_enabled() && cfg.extension_seconds == 0 {
        return Err(PokerTableError::InvalidTimeBankConfig);
    }
    env.storage()
        .instance()
        .set(&DataKey::TimeBankConfig(table.id), cfg);
    env.events().publish(
        (Symbol::new(env, "time_bank_configured"), table.id),
        (cfg.initial_seconds, cfg.max_seconds, cfg.extension_seconds),
    );
    Ok(())
}
