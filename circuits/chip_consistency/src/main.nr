/// Chip Count Consistency Circuit (Issue #227)
///
/// Proves that total chip supply is conserved across a table:
///
///   sum(player_stacks) + pot + rake == total_buy_ins
///
/// This is a fundamental invariant for poker tables: every chip that entered
/// the table via buy-ins must be accounted for in player stacks, the current
/// pot, or the rake balance. No chips can be created or destroyed.
///
/// # Inputs (all private)
/// - `player_stacks`: current chip stacks for each seat
/// - `num_players`: how many seats are occupied
/// - `pot`: total chips currently in the pot(s)
/// - `rake`: total rake collected this session
/// - `total_buy_ins`: cumulative buy-in amount for all players
///
/// # Public inputs
/// - `pub_total_buy_ins`: the on-chain total buy-in (for verification)
/// - `table_id`: identifier tying this proof to a specific table
///
/// # Returns
/// - `pub bool` — true if chips are consistent (always returns true on success)

global MAX_PLAYERS: u32 = 6;

fn main(
    player_stacks: [u32; MAX_PLAYERS],
    num_players: u32,
    pot: u32,
    rake: u32,
    total_buy_ins: u32,

    pub_total_buy_ins: pub u32,
    table_id: pub u32,
) -> pub bool {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // Verify the claimed total buy-ins matches the public value.
    assert(total_buy_ins == pub_total_buy_ins, "total buy-ins mismatch");

    // Sum all player stacks.
    let mut stacks_sum: u32 = 0;
    for i in 0..MAX_PLAYERS {
        if (i as u32) < num_players {
            stacks_sum += player_stacks[i];
        }
    }

    // Core invariant: stacks + pot + rake == total buy-ins.
    let accounted = stacks_sum + pot + rake;
    assert(accounted == total_buy_ins, "chip conservation violated");

    true
}
