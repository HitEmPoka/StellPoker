/// Side Pot Allocation Validity Circuit (Issue #219)
///
/// Proves that a claimed side-pot allocation is exactly the correct Texas
/// Hold'em derivation for a given set of player commitments and all-in / folded
/// flags. Given the (public) commitment, all-in, folded and declared pots, the
/// circuit:
///
///   1. Re-derives the correct side pots (amount per pot + per-seat eligibility)
///      from the commitments using the shared `pot::derive_side_pots` logic -
///      the same algorithm as `contracts/poker-table/src/pot.rs`.
///   2. Asserts the declared allocation (amounts and eligibility) matches the
///      derived one exactly, and that the number of declared pots matches.
///   3. Asserts chip conservation: the sum of all pot amounts equals the sum of
///      all player commitments (nothing is orphaned or created; folded players'
///      chips remain as dead money inside the pots they helped build).
///
/// This ties a table's on-chain side-pot records to the cryptographic guarantee
/// that only the chips a player matched can be won by others (Texas Hold'em rule),
/// and that every chip committed is accounted for.

use stellar_poker_lib::pot;

global MAX_PLAYERS: u32 = 6;
global MAX_POTS: u32 = 6;

fn main(
    // Hand state (public)
    num_players: pub u32,
    commitments: pub [u32; MAX_PLAYERS],
    all_in: pub [bool; MAX_PLAYERS],
    folded: pub [bool; MAX_PLAYERS],

    // Claimed allocation (public)
    num_pots: pub u32,
    declared_amounts: pub [u32; MAX_POTS],
    declared_eligible: pub [[bool; MAX_PLAYERS]; MAX_POTS],
) -> pub u32 {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");
    assert(num_pots <= MAX_POTS, "too many pots");

    // 1. Re-derive the correct side pots from the hand state.
    let (amounts, eligible, derived_count) = pot::derive_side_pots(commitments, all_in, folded, num_players);

    // 2. The declared allocation must match the derivation exactly.
    assert(num_pots == derived_count, "incorrect number of pots");
    for i in 0..MAX_POTS {
        assert(declared_amounts[i] == amounts[i], "pot amount mismatch");
        for p in 0..MAX_PLAYERS {
            assert(declared_eligible[i][p] == eligible[i][p], "pot eligibility mismatch");
        }
    }

    // 3. Chip conservation: sum of pot amounts == sum of player commitments.
    let mut total_pots: u32 = 0;
    for i in 0..MAX_POTS {
        total_pots += amounts[i];
    }
    let mut total_committed: u32 = 0;
    for p in 0..MAX_PLAYERS {
        total_committed += commitments[p];
    }
    assert(total_pots == total_committed, "chips not conserved");

    derived_count
}
