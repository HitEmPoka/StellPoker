//! Off-chain anti-chip-dumping detection (Issue #506).
//!
//! Chip dumping is the act of deliberately losing chips to a specific opponent.
//! This module builds a statistical picture from the normal hand-settlement
//! flow the coordinator already observes. It never looks at private hole cards
//! and never inspects authority data; it only aggregates public hand results
//! (who played, who won, and the size of the pot).
//!
//! For each ordered pair (P, C) the detector compares, over a bounded window:
//!
//! 1. `n`  = hands in which both P and C were dealt in.
//! 2. `k`  = hands among those where C won the pot while P was a non-winner
//!           (P lost that hand to C).
//! 3. `p`  = baseline probability that C wins a random window hand,
//!           i.e. `C_wins / hands_seen`.
//!
//! Under the null hypothesis (fair play) `k ~ Binomial(n, p)`. A one-tailed
//! z-score
//!
//! ```text
//!     z = (k - n*p) / sqrt(n*p*(1-p))
//! ```
//!
//! above `config.z_threshold` while the cumulative chips P has handed to C
//! clear `config.min_dump_chips` produces a `DumpSignal`.

use std::collections::{BTreeMap, VecDeque};

/// One settled hand, as observed by the coordinator after the on-chain pot
/// distribution. All fields are public information.
#[derive(Clone, Debug)]
pub struct HandOutcome {
    pub table_id: u32,
    pub session_id: u32,
    pub hand_number: u32,
    /// Base64 address of the player who won the pot (or `None` when the hand
    /// settled without a single winner, e.g. a timeout).
    pub winner: Option<String>,
    /// Base64 addresses of every player dealt into the hand.
    pub participants: Vec<String>,
    /// Gross chips awarded to the winner by this hand. When the coordinator
    /// could not observe the amounts (e.g. Soroban unconfigured) this is 0 and
    /// the amount gate is relaxed so the *pattern* still drives detection.
    pub pot: i128,
}

impl HandOutcome {
    /// Approximate per-loser donation when a hand has exactly one winner:
    /// split the pot evenly across the non-winning participants. Deterministic
    /// and purely public.
    pub fn donated_by_suspect(&self, suspect: &str, beneficiary: &str) -> i128 {
        if self.winner.as_deref() != Some(beneficiary) {
            return 0;
        }
        if !self.participants.iter().any(|p| p == suspect) {
            return 0;
        }
        let losers = self
            .participants
            .iter()
            .filter(|p| p.as_str() != beneficiary)
            .count();
        if losers == 0 {
            return self.pot;
        }
        (self.pot / losers as i128).max(0)
    }
}

/// Tunable parameters for the detector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DumpingConfig {
    /// How many most-recent settlement hands the detector retains.
    pub window_hands: usize,
    /// Minimum number of hands a pair must share before a signal is possible.
    pub min_encounters: usize,
    /// Minimum cumulative chips donated before a signal is surfaced.
    pub min_dump_chips: i128,
    /// z-score above which the observed loss rate is treated as suspicious.
    pub z_threshold: f64,
}

impl Default for DumpingConfig {
    fn default() -> Self {
        Self {
            window_hands: 200,
            min_encounters: 8,
            min_dump_chips: 100_000,
            z_threshold: 2.5,
        }
    }
}

/// A statistical alert raised by `DumpingDetector`.
#[derive(Clone, Debug, PartialEq)]
pub struct DumpSignal {
    pub table_id: u32,
    /// Suspected dumper.
    pub suspect: String,
    /// Suspected beneficiary.
    pub beneficiary: String,
    /// In-window chips the suspect donated to the beneficiary.
    pub donated: i128,
    /// Hands the pair shared (significance-test sample size).
    pub encounters: usize,
    /// Observed fraction of shared hands where the suspect lost to the
    /// beneficiary.
    pub observed_rate: f64,
    /// z-score of the deviation from the fair-play baseline.
    pub z_score: f64,
}

/// Per-directional-pair statistics accumulated over the window.
#[derive(Clone, Debug, Default)]
struct PairStats {
    encounters: usize,
    net_loss_hands: usize,
    donated: i128,
    table_id: u32,
}

/// Global window counters used to derive the fair-play baseline.
#[derive(Clone, Debug, Default)]
struct WindowStats {
    hands_seen: usize,
    wins_by_player: BTreeMap<String, usize>,
}

impl WindowStats {
    fn record(&mut self, h: &HandOutcome) {
        self.hands_seen += 1;
        if let Some(winner) = &h.winner {
            *self.wins_by_player.entry(winner.clone()).or_insert(0) += 1;
        }
    }

    /// Baseline probability that `person` wins a random window hand.
    fn win_rate(&self, person: &str) -> f64 {
        if self.hands_seen == 0 {
            return 0.0;
        }
        let wins = self.wins_by_player.get(person).copied().unwrap_or(0) as f64;
        wins / self.hands_seen as f64
    }
}

/// One-tailed z-score for the binomial approximation. Returns `f64::NAN` when
/// `n` is 0. `p` is clamped away from the degenerate 0.0/1.0 endpoints so a
/// beneficiary who wins every shared hand still yields a large (significant)
/// z-score instead of NaN.
fn binomial_z(k: usize, n: usize, p: f64) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    // Degenerate baseline: the beneficiary wins (or never wins) every window
    // hand. Any loss streak in the partner's direction is maximally
    // suspicious rather than statistically undefined.
    if p <= 1e-3 {
        return if k > 0 { 5.0 } else { f64::NAN };
    }
    if p >= 1.0 - 1e-3 {
        return if k == n { 5.0 } else { f64::NAN };
    }
    let nf = n as f64;
    let kf = k as f64;
    let denom = (nf * p * (1.0 - p)).sqrt();
    if denom == 0.0 {
        return f64::NAN;
    }
    (kf - nf * p) / denom
}

/// Streaming detector of chip-dumping patterns.
///
/// State is intentionally cheap: a bounded FIFO of recent `HandOutcome`s and
/// lazily-recomputed aggregation maps built only when `reports()` is called.
#[derive(Clone, Debug)]
pub struct DumpingDetector {
    config: DumpingConfig,
    hands: VecDeque<HandOutcome>,
}

impl DumpingDetector {
    pub fn new(config: DumpingConfig) -> Self {
        Self {
            config,
            hands: VecDeque::with_capacity(config.window_hands),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(DumpingConfig::default())
    }

    /// Record a settled hand. Keeps the window bounded.
    pub fn observe(&mut self, hand: HandOutcome) {
        self.hands.push_back(hand);
        while self.hands.len() > self.config.window_hands {
            self.hands.pop_front();
        }
    }

    pub fn hand_count(&self) -> usize {
        self.hands.len()
    }

    pub fn config(&self) -> &DumpingConfig {
        &self.config
    }

    /// Recompute pair statistics + baseline from the current window and return
    /// every pair that exceeds the significance threshold, highest z-score
    /// first.
    pub fn reports(&self) -> Vec<DumpSignal> {
        let mut window = WindowStats::default();
        let mut pairs: BTreeMap<(String, String), PairStats> = BTreeMap::new();

        for h in &self.hands {
            window.record(h);
            let participants = &h.participants;
            for i in 0..participants.len() {
                for j in (i + 1)..participants.len() {
                    let a = &participants[i];
                    let b = &participants[j];
                    // Count the shared hand for both directions so each
                    // directional pair's denominator is the number of hands
                    // where BOTH players were dealt in.
                    self.touch_pair(&mut pairs, h, a, b);
                    self.touch_pair(&mut pairs, h, b, a);
                }
            }
        }

        let cfg = self.config;
        let mut out = Vec::new();
        for ((suspect, beneficiary), stats) in &pairs {
            if stats.encounters < cfg.min_encounters {
                continue;
            }
            // When every observed hand lacked amounts, don't gate on chips.
            let amounts_known = window.hands_seen > 0 && stats.donated > 0;
            if amounts_known && stats.donated < cfg.min_dump_chips {
                continue;
            }
            let p = window.win_rate(beneficiary);
            let z = binomial_z(stats.net_loss_hands, stats.encounters, p);
            if z.is_nan() || z < cfg.z_threshold {
                continue;
            }
            let observed_rate = stats.net_loss_hands as f64 / stats.encounters as f64;
            out.push(DumpSignal {
                table_id: stats.table_id,
                suspect: suspect.clone(),
                beneficiary: beneficiary.clone(),
                donated: stats.donated,
                encounters: stats.encounters,
                observed_rate,
                z_score: z,
            });
        }
        out.sort_by(|l, r| {
            r.z_score
                .partial_cmp(&l.z_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Update the (directional) pair bucket for a shared hand. The hand counts
    /// toward the pair's sample each time both players were dealt in; a loss is
    /// credited only when `winner` took the pot and `loser` was a participant.
    fn touch_pair(
        &self,
        pairs: &mut BTreeMap<(String, String), PairStats>,
        h: &HandOutcome,
        loser: &str,
        winner: &str,
    ) {
        if loser.eq_ignore_ascii_case(winner) {
            return;
        }
        let pr = pairs
            .entry((loser.to_string(), winner.to_string()))
            .or_default();
        pr.encounters += 1;
        pr.table_id = h.table_id;
        if h.winner.as_deref() == Some(winner) {
            pr.net_loss_hands += 1;
            pr.donated += h.donated_by_suspect(loser, winner).max(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_small() -> DumpingConfig {
        DumpingConfig {
            window_hands: 20,
            min_encounters: 5,
            min_dump_chips: 1_000,
            z_threshold: 1.5,
        }
    }

    fn outcome(n: u32, winner: &str, participants: Vec<&str>, pot: i128) -> HandOutcome {
        HandOutcome {
            table_id: 1,
            session_id: 1,
            hand_number: n,
            winner: Some(winner.to_string()),
            participants: participants.into_iter().map(String::from).collect(),
            pot,
        }
    }

    #[test]
    fn consistent_loser_to_same_winner_is_signalled() {
        let mut d = DumpingDetector::new(cfg_small());
        // Victim and Ben play every hand; Ben always takes the pot and Victim
        // is always among the losers. Alice wins a couple of small pots so Ben
        // is not the only net-positive player (keeps the baseline bounded).
        for n in 1..=12u32 {
            d.observe(outcome(n, "Ben", vec!["Victim", "Ben", "Alice"], 200_000));
        }
        for n in 13..=16u32 {
            d.observe(outcome(n, "Alice", vec!["Victim", "Alice"], 30_000));
        }

        let reports = d.reports();
        assert!(
            reports
                .iter()
                .any(|s| { s.suspect == "Victim" && s.beneficiary == "Ben" && s.z_score >= 1.5 }),
            "expected a dump signal for Victim->Ben, got {:?}",
            reports
        );
    }

    #[test]
    fn balanced_play_produces_no_signal() {
        let mut d = DumpingDetector::new(cfg_small());
        // The pair splits pots 50/50 across shared hands.
        for n in 1..=20u32 {
            if n % 2 == 0 {
                d.observe(outcome(n, "A", vec!["A", "B"], 20_000));
            } else {
                d.observe(outcome(n, "B", vec!["A", "B"], 20_000));
            }
        }
        assert!(
            d.reports().is_empty(),
            "balanced window must not produce signals: {:?}",
            d.reports()
        );
    }

    #[test]
    fn window_is_bounded() {
        let mut d = DumpingDetector::new(cfg_small());
        for n in 1..=200u32 {
            d.observe(outcome(n, "W", vec!["W", "L"], 2_000));
        }
        assert_eq!(d.hand_count(), cfg_small().window_hands);
    }

    #[test]
    fn below_min_donated_is_not_signalled() {
        let mut d = DumpingDetector::new(cfg_small());
        for n in 1..=8u32 {
            d.observe(outcome(n, "Ben", vec!["Ben", "Victim"], 100));
        }
        // Total donated ~400 < min_dump_chips 1000 -> no signal even though the
        // loss pattern is extreme.
        assert!(d.reports().is_empty(), "tiny pots must be ignored");
    }

    #[test]
    fn unknown_amounts_still_signal_clear_patterns() {
        let mut d = DumpingDetector::new(cfg_small());
        // pot == 0 everywhere (Soroban unconfigured) — pattern alone signals.
        for n in 1..=10u32 {
            d.observe(outcome(n, "Ben", vec!["Victim", "Ben"], 0));
        }
        let reports = d.reports();
        assert!(
            reports
                .iter()
                .any(|s| { s.suspect == "Victim" && s.beneficiary == "Ben" && s.donated == 0 }),
            "pattern must signal without chip data: {:?}",
            reports
        );
    }

    #[test]
    fn zero_window_has_no_signals() {
        let d = DumpingDetector::new(cfg_small());
        assert!(d.reports().is_empty());
    }
}
