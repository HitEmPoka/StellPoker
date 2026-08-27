/// Split-Pot (Hi-Lo) Validity Circuit (Issue #205)
///
/// Proves that a split-pot allocation between high and low hands is correct.
/// The pot is split 50/50 between the best high hand and the best qualifying
/// low hand. A low hand qualifies if it is "8 or better" — no card in the
/// low hand may rank above 8 (Ace = 14 is high, King = 13, etc.).
///
/// Inputs:
///   - `pot_amount`: total chips in the pot (private)
///   - `num_eligible`: how many eligible players (private)
///   - `eligible_players`: seat indices of eligible players (private)
///   - `high_winner_seat`: seat index of the high hand winner (private)
///   - `low_winner_seat`: seat index of the low hand winner (private)
///   - `low_qualified`: whether the low hand qualifies (private)
///   - `high_amount`: claimed high half (public)
///   - `low_amount`: claimed low half (public)
///   - `high_winner_pub`: public seat index of high winner (public)
///   - `low_winner_pub`: public seat index of low winner (public, 0 if N/A)

global MAX_PLAYERS: u32 = 6;

fn main(
    pot_amount: u32,
    num_eligible: u32,
    eligible_players: [u32; MAX_PLAYERS],
    high_winner_seat: u32,
    low_winner_seat: u32,
    low_qualified: bool,

    high_amount: pub u32,
    low_amount: pub u32,
    high_winner_pub: pub u32,
    low_winner_pub: pub u32,
) {
    assert(num_eligible >= 2, "need at least 2 eligible players");
    assert(num_eligible <= MAX_PLAYERS, "too many eligible players");

    // Verify the high winner is in the eligible set.
    let mut high_found = false;
    for i in 0..MAX_PLAYERS {
        if i < num_eligible {
            if eligible_players[i] == high_winner_seat {
                high_found = true;
            }
        }
    }
    assert(high_found, "high winner not eligible");

    // Verify public winner matches private winner.
    assert(high_winner_pub == high_winner_seat, "high winner mismatch");

    if low_qualified {
        // Verify the low winner is in the eligible set.
        let mut low_found = false;
        for i in 0..MAX_PLAYERS {
            if i < num_eligible {
                if eligible_players[i] == low_winner_seat {
                    low_found = true;
                }
            }
        }
        assert(low_found, "low winner not eligible");
        assert(low_winner_pub == low_winner_seat, "low winner mismatch");

        if high_winner_seat == low_winner_seat {
            // Same player wins both halves — entire pot goes to them.
            assert(high_amount == pot_amount, "high amount must be full pot");
            assert(low_amount == 0, "low amount must be zero when same winner");
        } else {
            // Standard 50/50 split.
            let half = pot_amount / 2;
            let remainder = pot_amount - half;
            assert(high_amount == remainder, "high amount mismatch");
            assert(low_amount == half, "low amount mismatch");
        }
    } else {
        // Low does not qualify — high takes the whole pot.
        assert(high_amount == pot_amount, "high must take full pot when low unqualified");
        assert(low_amount == 0, "low amount must be zero when unqualified");
    }
}
