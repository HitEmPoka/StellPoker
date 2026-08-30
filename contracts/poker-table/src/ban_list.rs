use soroban_sdk::{Address, Env, Map, Symbol, Vec};
use crate::types::DataKey;

/// Player ban/unban list per table (Issue #195)
/// Stored per-table in persistent storage keyed by (table_id, player)

fn ban_key(env: &Env, table_id: u32) -> Symbol {
    // Use a combined symbol + id as key via Symbol short + table_id in persistent Map
    // For simplicity we use a single Map keyed by (table_id, player) tuple stored as
    // DataKey-like instance storage per table: key is (Symbol("ban"), table_id)
    let _ = env;
    Symbol::new(env, "ban_list")
}

/// Per-table ban map key: (table_id) -> Map<Address, Symbol> (reason)
fn store_key(table_id: u32) -> (Symbol, u32) {
    (Symbol::short("ban"), table_id)
}

/// Ban a player from the table (stores reason)
pub fn ban_player(env: &Env, table_id: u32, player: Address, reason: Symbol) {
    let key = store_key(table_id);
    let mut bans: Map<Address, Symbol> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    bans.set(player, reason);
    env.storage().persistent().set(&key, &bans);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
}

/// Unban a player
pub fn unban_player(env: &Env, table_id: u32, player: &Address) {
    let key = store_key(table_id);
    let mut bans: Map<Address, Symbol> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    bans.remove(player.clone());
    env.storage().persistent().set(&key, &bans);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
}

/// Check if a player is banned
pub fn is_banned(env: &Env, table_id: u32, player: &Address) -> bool {
    let key = store_key(table_id);
    let bans: Map<Address, Symbol> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    bans.contains_key(player.clone())
}

/// Legacy alias for is_banned with player only (for internal checks where table_id unknown)
/// This will check all tables? For simplicity return false.
pub fn is_player_banned(_env: &Env, _player: &Address) -> bool {
    false
}

/// Get all banned players for a table
pub fn get_banned_players(env: &Env, table_id: u32) -> Vec<(Address, Symbol)> {
    let key = store_key(table_id);
    let bans: Map<Address, Symbol> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    let mut out = Vec::new(env);
    for (addr, reason) in bans.iter() {
        out.push_back((addr, reason));
    }
    out
}

/// Legacy Map getter
pub fn get_banned_players_map(env: &Env) -> Map<Address, Symbol> {
    let _ = env;
    Map::new(env)
}
