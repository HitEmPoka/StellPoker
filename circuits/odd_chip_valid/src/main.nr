/// Odd Chip Allocation Validity Circuit
///
/// Proves that claimed odd chip payouts in a split pot correctly reflect
/// Texas Hold'em rules and match the on-chain poker contract specification
/// (`contracts/poker-table/src/pot.rs` `distribute_pots_with_ties`).
///
/// When a pot is split among multiple tied winners and cannot be divided
/// evenly, odd chips (remainders) must be awarded deterministically:
///
/// 1. Base Share: floor(pot_amount / W) chips to each of W tied winners.
/// 2. Remainder: R = pot_amount % W odd chips to distribute (0 <= R < W).
/// 3. Priority Order:
///    - `RULE_LEFT_OF_BUTTON (0)`: Odd chips are awarded one-by-one clockwise
///      starting from the seat immediately to the left of the dealer button
///      (`(dealer_seat + 1) % num_players`).
///    - `RULE_EARLIEST_SEAT (1)`: Odd chips are awarded in ascending seat index
///      order (earliest seat first), matching `contracts/poker-table/src/pot.rs`.
/// 4. Chip Conservation: Sum of all payouts equals `pot_amount` exactly.
/// 5. Non-winners: All seats not marked as tied winners receive exactly 0 chips.

use stellar_poker_lib::pot;

global MAX_PLAYERS: u32 = 6;

fn main(
    // Table & Split Pot state (public)
    pot_amount: pub u32,
    num_players: pub u32,
    dealer_seat: pub u32,
    tied_winners: pub [bool; MAX_PLAYERS],
    rule: pub u32,

    // Claimed payouts (public)
    claimed_payouts: pub [u32; MAX_PLAYERS],
) -> pub (u32, u32, [u32; MAX_PLAYERS]) {
    let (base_share, remainder) = pot::verify_odd_chip_distribution(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    (base_share, remainder, claimed_payouts)
}

#[test]
fn test_three_way_split_zero_remainder() {
    // 3-way split: 300 chips divided evenly among 3 winners (seats 0, 1, 2)
    // 300 / 3 = 100, remainder 0
    let pot_amount = 300;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [100, 100, 100, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 100);
    assert(remainder == 0);
    assert(payouts[0] == 100);
    assert(payouts[1] == 100);
    assert(payouts[2] == 100);
}

#[test]
fn test_three_way_split_one_odd_chip_dealer_seat_zero() {
    // 3-way split: 100 chips among seats 0, 1, 2. Dealer at seat 0.
    // 100 / 3 = 33 r 1.
    // Left of button is seat 1 (dist 0) -> awarded the odd chip (+1 = 34).
    // Seats 2 and 0 receive 33 each.
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 34, 33, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 1);
    assert(payouts[0] == 33);
    assert(payouts[1] == 34);
    assert(payouts[2] == 33);
}

#[test]
fn test_three_way_split_one_odd_chip_dealer_seat_one() {
    // 3-way split: 100 chips among seats 0, 1, 2. Dealer at seat 1.
    // Left of button is seat 2 (dist 0) -> awarded the odd chip (34).
    // Seats 0 and 1 receive 33 each.
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 1;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 33, 34, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 1);
    assert(payouts[0] == 33);
    assert(payouts[1] == 33);
    assert(payouts[2] == 34);
}

#[test]
fn test_three_way_split_one_odd_chip_dealer_seat_two() {
    // 3-way split: 100 chips among seats 0, 1, 2. Dealer at seat 2.
    // Left of button is seat 0 (dist 0) -> awarded the odd chip (34).
    // Seats 1 and 2 receive 33 each.
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 2;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [34, 33, 33, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 1);
    assert(payouts[0] == 34);
    assert(payouts[1] == 33);
    assert(payouts[2] == 33);
}

#[test]
fn test_three_way_split_two_odd_chips_dealer_seat_zero() {
    // 3-way split: 101 chips among seats 0, 1, 2. Dealer at seat 0.
    // 101 / 3 = 33 r 2.
    // 1st odd chip -> seat 1 (left of button, dist 0) = 34
    // 2nd odd chip -> seat 2 (next clockwise, dist 1) = 34
    // Seat 0 (button, dist 2) receives base share = 33
    // Total: 34 + 34 + 33 = 101 chips (exact conservation)
    let pot_amount = 101;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 34, 34, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 33);
    assert(payouts[1] == 34);
    assert(payouts[2] == 34);
}

#[test]
fn test_three_way_split_two_odd_chips_dealer_seat_two() {
    // 3-way split: 101 chips among seats 0, 1, 2. Dealer at seat 2.
    // Left of button is seat 0 (dist 0), then seat 1 (dist 1), then seat 2 (dist 2).
    // Remainder 2 chips -> seat 0 gets 34, seat 1 gets 34, seat 2 gets 33.
    let pot_amount = 101;
    let num_players = 3;
    let dealer_seat = 2;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [34, 34, 33, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 34);
    assert(payouts[1] == 34);
    assert(payouts[2] == 33);
}

#[test]
fn test_three_way_split_two_odd_chips_dealer_seat_one() {
    // 3-way split: 101 chips among seats 0, 1, 2. Dealer at seat 1.
    // Left of button is seat 2 (dist 0), then seat 0 (dist 1), then seat 1 (dist 2).
    // Remainder 2 chips -> seat 2 gets 34, seat 0 gets 34, seat 1 gets 33.
    let pot_amount = 101;
    let num_players = 3;
    let dealer_seat = 1;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [34, 33, 34, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 34);
    assert(payouts[1] == 33);
    assert(payouts[2] == 34);
}

#[test]
fn test_three_way_split_earliest_seat_rule() {
    // 3-way split under RULE_EARLIEST_SEAT (matching contracts/poker-table/src/pot.rs)
    // 101 chips among seats 0, 1, 2.
    // Remainder 2 chips awarded to earliest seats 0 and 1 (34 each), seat 2 gets 33.
    let pot_amount = 101;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_EARLIEST_SEAT;
    let claimed_payouts = [34, 34, 33, 0, 0, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 34);
    assert(payouts[1] == 34);
    assert(payouts[2] == 33);
}

#[test]
fn test_three_way_split_non_contiguous_five_players() {
    // 5 players table (seats 0..4). Dealer at seat 2.
    // Tied winners are seats 1, 3, 4 (3-way tie).
    // Left of button (seat 2) is seat 3 (dist 0), then seat 4 (dist 1), then seat 1 (dist 3).
    // Pot = 101 (33 r 2).
    // Seat 3 gets 34, seat 4 gets 34, seat 1 gets 33. Seats 0 and 2 get 0.
    let pot_amount = 101;
    let num_players = 5;
    let dealer_seat = 2;
    let tied_winners = [false, true, false, true, true, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [0, 33, 0, 34, 34, 0];

    let (base_share, remainder, payouts) = main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );

    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 0);
    assert(payouts[1] == 33);
    assert(payouts[2] == 0);
    assert(payouts[3] == 34);
    assert(payouts[4] == 34);
    assert(payouts[5] == 0);
}

#[test(should_fail)]
fn test_invalid_odd_chip_awarded_to_wrong_seat() {
    // Dealer at seat 0, pot 100 (remainder 1).
    // Seat 1 should receive 34 (left of button), but seat 0 (button) claims 34 instead.
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [34, 33, 33, 0, 0, 0]; // Invalid: odd chip given to button

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}

#[test(should_fail)]
fn test_invalid_chip_conservation_extra_chip() {
    // Pot is 100, but claimed total is 34 + 34 + 33 = 101 (conservation violation)
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 34, 34, 0, 0, 0];

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}

#[test(should_fail)]
fn test_invalid_chip_conservation_missing_chip() {
    // Pot is 100, but claimed total is 33 + 33 + 33 = 99 (remainder chip lost)
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 33, 33, 0, 0, 0];

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}

#[test(should_fail)]
fn test_invalid_non_tied_winner_receives_chips() {
    // Seat 3 is not a tied winner, but claims chips
    let pot_amount = 100;
    let num_players = 4;
    let dealer_seat = 0;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 33, 33, 1, 0, 0];

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}

#[test(should_fail)]
fn test_invalid_only_one_winner() {
    // Split pot requires at least 2 tied winners
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 0;
    let tied_winners = [true, false, false, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [100, 0, 0, 0, 0, 0];

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}

#[test(should_fail)]
fn test_invalid_dealer_seat_out_of_bounds() {
    // dealer_seat is 3 for 3 players (valid seats are 0, 1, 2)
    let pot_amount = 100;
    let num_players = 3;
    let dealer_seat = 3;
    let tied_winners = [true, true, true, false, false, false];
    let rule = pot::RULE_LEFT_OF_BUTTON;
    let claimed_payouts = [33, 34, 33, 0, 0, 0];

    main(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
        claimed_payouts,
    );
}
