use crate::types::*;
use soroban_sdk::{Address, Env, Map, Symbol};

/// Player ban/unban list for table owners
/// Issue #195

const BAN_LIST: Symbol = Symbol::short("BANLIST");

/// Ban a player from the table (owner only)
pub fn ban_player(
    env: &Env,
    table: &TableState,
    caller: &Address,
    player: Address,
) -> Result<(), PokerTableError> {
    // Only table admin can ban players
    if caller != &table.admin {
        return Err(PokerTableError::NotAuthorizedCommittee);
    }

    caller.require_auth();

    let mut ban_list: Map<Address, bool> = env
        .storage()
        .persistent()
        .get(&BAN_LIST)
        .unwrap_or(Map::new(env));

    ban_list.set(player.clone(), true);
    env.storage().persistent().set(&BAN_LIST, &ban_list);

    // Emit event
    env.events()
        .publish((Symbol::new(env, "player_banned"),), player);

    Ok(())
}

/// Unban a player from the table (owner only)
pub fn unban_player(
    env: &Env,
    table: &TableState,
    caller: &Address,
    player: Address,
) -> Result<(), PokerTableError> {
    // Only table admin can unban players
    if caller != &table.admin {
        return Err(PokerTableError::NotAuthorizedCommittee);
    }

    caller.require_auth();

    let mut ban_list: Map<Address, bool> = env
        .storage()
        .persistent()
        .get(&BAN_LIST)
        .unwrap_or(Map::new(env));

    ban_list.set(player.clone(), false);
    env.storage().persistent().set(&BAN_LIST, &ban_list);

    // Emit event
    env.events()
        .publish((Symbol::new(env, "player_unbanned"),), player);

    Ok(())
}

/// Check if a player is banned
pub fn is_player_banned(env: &Env, player: &Address) -> bool {
    let ban_list: Map<Address, bool> = env
        .storage()
        .persistent()
        .get(&BAN_LIST)
        .unwrap_or(Map::new(env));

    ban_list.get(player.clone()).unwrap_or(false)
}

/// Get all banned players
pub fn get_banned_players(env: &Env) -> Map<Address, bool> {
    env.storage()
        .persistent()
        .get(&BAN_LIST)
        .unwrap_or(Map::new(env))
}
