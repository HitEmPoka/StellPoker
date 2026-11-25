//! Feldman verifiable secret sharing for the key enrollment ceremony (#884).
//!
//! Plain Shamir sharing gives a committee member no way to tell whether the
//! share they were handed is genuine: a faulty or malicious dealer can distribute
//! shares that do not lie on one polynomial, and nobody finds out until a quorum
//! tries to reconstruct and fails. Feldman's scheme closes that gap. Alongside
//! the shares the dealer publishes a *commitment* to every polynomial
//! coefficient, `C_k = a_k · G`, and anyone holding a share `(x, y)` can check
//!
//! ```text
//!     y · G  ==  Σ_k  x^k · C_k
//! ```
//!
//! without learning anything about the other shares.
//!
//! ## Group and secret
//!
//! Commitments live in the Ristretto255 group (prime order `l`, no cofactor
//! pitfalls); shares and coefficients are scalars mod `l`. The shared secret is
//! one **full-entropy scalar** `s`, and the node's ed25519 seed is derived from
//! it with [`derive_seed`]. The commitment `C_0 = s · G` is public by design, so
//! the secret must be a uniformly random scalar (about 126 bits of discrete-log
//! security). Sharing the 256-bit ed25519 seed directly, for example as two
//! 128-bit limbs, would publish `g^limb` for a 128-bit exponent, which
//! generic discrete-log attacks recover in roughly 2^64 steps. Hashing a
//! full-size scalar into the seed avoids that.
//!
//! ## Transcript hash
//!
//! The commitments, together with the ceremony parameters, are bound into one
//! SHA-256 [`transcript_hash`]. That 32-byte value is what the
//! `committee-registry` contract stores for the member, so a node can check at
//! session start that the commitments it was given are the ones the committee
//! agreed on, and then check its share against them ([`verify_before_deal`]).

use std::fmt;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Identifier written into share files and the public commitments artefact.
pub const SCHEME: &str = "feldman-ristretto255-v1";

/// Byte length of a share value, a secret scalar and a compressed commitment.
pub const SCALAR_LEN: usize = 32;

const TRANSCRIPT_DOMAIN: &[u8] = b"stellpoker/feldman-transcript/v1";
const SEED_DOMAIN: &[u8] = b"stellpoker/node-seed/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeldmanError {
    /// `threshold`/`shares` outside `2 <= threshold <= shares <= 254`.
    InvalidParameters { threshold: u8, shares: u8 },
    /// The secret is not a canonical (reduced) scalar.
    SecretOutOfRange,
    /// Share index 0 is the secret itself and is never issued.
    ZeroIndex,
    DuplicateIndex(u8),
    /// The share value is not 32 bytes or not a canonical scalar.
    MalformedShare { index: u8 },
    /// Wrong number of points, or a point that is not a valid encoding.
    MalformedCommitments(String),
    /// The share does not satisfy the commitments.
    ShareMismatch { index: u8 },
    NotEnoughShares { needed: u8, got: usize },
    /// The shares verify individually but do not rebuild the committed secret.
    ReconstructionMismatch,
    /// The commitments do not hash to the value recorded on-chain.
    TranscriptMismatch,
}

impl fmt::Display for FeldmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParameters { threshold, shares } => write!(
                f,
                "invalid sharing parameters: need 2 <= threshold ({threshold}) <= shares \
                 ({shares}) <= 254"
            ),
            Self::SecretOutOfRange => write!(f, "secret is not a canonical scalar"),
            Self::ZeroIndex => write!(f, "share index 0 is not a valid share"),
            Self::DuplicateIndex(index) => write!(f, "duplicate share index {index}"),
            Self::MalformedShare { index } => {
                write!(f, "share {index} is not a canonical 32-byte scalar")
            }
            Self::MalformedCommitments(why) => write!(f, "malformed commitments: {why}"),
            Self::ShareMismatch { index } => {
                write!(f, "share {index} does not match the published commitments")
            }
            Self::NotEnoughShares { needed, got } => {
                write!(f, "need at least {needed} shares, got {got}")
            }
            Self::ReconstructionMismatch => write!(
                f,
                "shares do not reconstruct the secret the commitments were made for"
            ),
            Self::TranscriptMismatch => write!(
                f,
                "commitment transcript hash does not match the one recorded on-chain"
            ),
        }
    }
}

impl std::error::Error for FeldmanError {}

/// One member's share: a Shamir x-coordinate and the polynomial value there.
#[derive(Clone)]
pub struct Share {
    index: u8,
    value: [u8; SCALAR_LEN],
}

impl Share {
    /// Builds a share from raw bytes, checking it is a canonical scalar.
    pub fn new(index: u8, value: &[u8]) -> Result<Self, FeldmanError> {
        if index == 0 {
            return Err(FeldmanError::ZeroIndex);
        }
        let value: [u8; SCALAR_LEN] = value
            .try_into()
            .map_err(|_| FeldmanError::MalformedShare { index })?;
        let share = Self { index, value };
        share.scalar()?;
        Ok(share)
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn as_bytes(&self) -> &[u8; SCALAR_LEN] {
        &self.value
    }

    fn scalar(&self) -> Result<Scalar, FeldmanError> {
        let parsed: Option<Scalar> = Scalar::from_canonical_bytes(self.value).into();
        parsed.ok_or(FeldmanError::MalformedShare { index: self.index })
    }
}

impl fmt::Debug for Share {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Share {{ index: {}, value: [REDACTED] }}", self.index)
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// Public commitments `C_0 .. C_{t-1}` to the sharing polynomial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitments {
    points: Vec<[u8; SCALAR_LEN]>,
}

impl Commitments {
    /// Wraps compressed points, checking each one is a valid Ristretto encoding.
    /// The threshold is the number of points.
    pub fn from_points(points: Vec<[u8; SCALAR_LEN]>) -> Result<Self, FeldmanError> {
        if points.len() < 2 || points.len() > 254 {
            return Err(FeldmanError::MalformedCommitments(format!(
                "expected between 2 and 254 points, got {}",
                points.len()
            )));
        }
        let commitments = Self { points };
        commitments.decompress()?;
        Ok(commitments)
    }

    pub fn from_hex<S: AsRef<str>>(points: &[S]) -> Result<Self, FeldmanError> {
        let mut decoded = Vec::with_capacity(points.len());
        for (position, point) in points.iter().enumerate() {
            let bytes = hex::decode(point.as_ref()).map_err(|e| {
                FeldmanError::MalformedCommitments(format!("point {position} is not hex: {e}"))
            })?;
            let bytes: [u8; SCALAR_LEN] = bytes.try_into().map_err(|_| {
                FeldmanError::MalformedCommitments(format!("point {position} is not 32 bytes"))
            })?;
            decoded.push(bytes);
        }
        Self::from_points(decoded)
    }

    pub fn to_hex(&self) -> Vec<String> {
        self.points.iter().map(hex::encode).collect()
    }

    /// The number of shares needed to reconstruct (the polynomial degree + 1).
    pub fn threshold(&self) -> u8 {
        self.points.len() as u8
    }

    pub fn points(&self) -> &[[u8; SCALAR_LEN]] {
        &self.points
    }

    fn decompress(&self) -> Result<Vec<RistrettoPoint>, FeldmanError> {
        self.points
            .iter()
            .enumerate()
            .map(|(position, bytes)| {
                CompressedRistretto(*bytes).decompress().ok_or_else(|| {
                    FeldmanError::MalformedCommitments(format!(
                        "point {position} is not a valid Ristretto encoding"
                    ))
                })
            })
            .collect()
    }
}

/// What the transcript hash binds the commitments to.
#[derive(Clone, Copy, Debug)]
pub struct TranscriptContext<'a> {
    pub node_id: u32,
    /// The node's Stellar public key (`G...`), i.e. the registry `member`.
    pub public_key: &'a str,
    /// Total shares issued in the ceremony.
    pub shares: u8,
}

/// SHA-256 over the ceremony parameters and every commitment, in order. This is
/// the value stored on-chain in the committee registry.
pub fn transcript_hash(commitments: &Commitments, ctx: &TranscriptContext) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TRANSCRIPT_DOMAIN);
    hasher.update(ctx.node_id.to_be_bytes());
    hasher.update((ctx.public_key.len() as u32).to_be_bytes());
    hasher.update(ctx.public_key.as_bytes());
    hasher.update([commitments.threshold(), ctx.shares]);
    for point in commitments.points() {
        hasher.update(point);
    }
    hasher.finalize().into()
}

/// Derives the node's ed25519 seed from the shared secret scalar.
pub fn derive_seed(secret: &[u8; SCALAR_LEN]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SEED_DOMAIN);
    hasher.update(secret);
    hasher.finalize().into()
}

/// A fresh uniformly random non-zero secret scalar, as canonical bytes.
pub fn random_secret<R: RngCore>(rng: &mut R) -> [u8; SCALAR_LEN] {
    random_scalar(rng).to_bytes()
}

fn random_scalar<R: RngCore>(rng: &mut R) -> Scalar {
    loop {
        let mut wide = [0u8; 64];
        rng.fill_bytes(&mut wide);
        let scalar = Scalar::from_bytes_mod_order_wide(&wide);
        wide.zeroize();
        if scalar != Scalar::ZERO {
            return scalar;
        }
    }
}

fn index_scalar(index: u8) -> Result<Scalar, FeldmanError> {
    if index == 0 {
        return Err(FeldmanError::ZeroIndex);
    }
    Ok(Scalar::from(u64::from(index)))
}

/// `a_0 + a_1 x + ... + a_{t-1} x^{t-1}` by Horner's rule.
fn eval_scalars(coefficients: &[Scalar], x: &Scalar) -> Scalar {
    let mut acc = Scalar::ZERO;
    for coefficient in coefficients.iter().rev() {
        acc = acc * x + coefficient;
    }
    acc
}

/// `Σ x^k · C_k` by Horner's rule. `points` is never empty here (t >= 2).
fn eval_points(points: &[RistrettoPoint], x: &Scalar) -> RistrettoPoint {
    let (last, rest) = points.split_last().expect("at least two commitments");
    let mut acc = *last;
    for point in rest.iter().rev() {
        acc = acc * *x + *point;
    }
    acc
}

/// Splits `secret` into `shares` Feldman shares, any `threshold` of which
/// reconstruct it, and returns the public commitments.
pub fn deal<R: RngCore>(
    secret: &[u8; SCALAR_LEN],
    threshold: u8,
    shares: u8,
    rng: &mut R,
) -> Result<(Vec<Share>, Commitments), FeldmanError> {
    if threshold < 2 || shares < threshold || shares == u8::MAX {
        return Err(FeldmanError::InvalidParameters { threshold, shares });
    }
    let constant: Option<Scalar> = Scalar::from_canonical_bytes(*secret).into();
    let constant = constant.ok_or(FeldmanError::SecretOutOfRange)?;

    let mut coefficients = Vec::with_capacity(threshold as usize);
    coefficients.push(constant);
    for _ in 1..threshold {
        coefficients.push(random_scalar(rng));
    }

    let points: Vec<[u8; SCALAR_LEN]> = coefficients
        .iter()
        .map(|c| (RISTRETTO_BASEPOINT_POINT * *c).compress().to_bytes())
        .collect();

    let mut dealt = Vec::with_capacity(shares as usize);
    for index in 1..=shares {
        let y = eval_scalars(&coefficients, &Scalar::from(u64::from(index)));
        dealt.push(Share {
            index,
            value: y.to_bytes(),
        });
    }
    coefficients.zeroize();

    Ok((dealt, Commitments { points }))
}

/// Checks one share against the commitments.
pub fn verify_share(commitments: &Commitments, share: &Share) -> Result<(), FeldmanError> {
    let x = index_scalar(share.index)?;
    let y = share.scalar()?;
    let points = commitments.decompress()?;
    if RISTRETTO_BASEPOINT_POINT * y == eval_points(&points, &x) {
        Ok(())
    } else {
        Err(FeldmanError::ShareMismatch { index: share.index })
    }
}

/// Indices of every share that fails verification (empty means all are good).
pub fn find_invalid_shares(commitments: &Commitments, shares: &[Share]) -> Vec<u8> {
    shares
        .iter()
        .filter(|share| verify_share(commitments, share).is_err())
        .map(Share::index)
        .collect()
}

/// Lagrange interpolation at `x = 0`. Does **not** check the shares against
/// anything: wrong or too few shares produce a wrong secret rather than an
/// error. Prefer [`reconstruct_verified`].
pub fn reconstruct(shares: &[Share]) -> Result<[u8; SCALAR_LEN], FeldmanError> {
    if shares.is_empty() {
        return Err(FeldmanError::NotEnoughShares { needed: 1, got: 0 });
    }
    let mut points = Vec::with_capacity(shares.len());
    for (position, share) in shares.iter().enumerate() {
        let x = index_scalar(share.index)?;
        if shares[..position].iter().any(|s| s.index == share.index) {
            return Err(FeldmanError::DuplicateIndex(share.index));
        }
        points.push((x, share.scalar()?));
    }

    let mut secret = Scalar::ZERO;
    for (i, (x_i, y_i)) in points.iter().enumerate() {
        // λ_i = Π_{j != i} x_j / (x_j - x_i)
        let mut lambda = Scalar::ONE;
        for (j, (x_j, _)) in points.iter().enumerate() {
            if i != j {
                lambda *= x_j * (x_j - x_i).invert();
            }
        }
        secret += lambda * y_i;
    }
    Ok(secret.to_bytes())
}

/// Verifies every share, requires a quorum, and checks the reconstructed
/// secret against the first commitment `C_0`.
pub fn reconstruct_verified(
    commitments: &Commitments,
    shares: &[Share],
) -> Result<[u8; SCALAR_LEN], FeldmanError> {
    let needed = commitments.threshold();
    if shares.len() < needed as usize {
        return Err(FeldmanError::NotEnoughShares {
            needed,
            got: shares.len(),
        });
    }
    for share in shares {
        verify_share(commitments, share)?;
    }
    let secret = reconstruct(shares)?;
    let scalar: Option<Scalar> = Scalar::from_canonical_bytes(secret).into();
    let scalar = scalar.ok_or(FeldmanError::SecretOutOfRange)?;
    let committed = CompressedRistretto(commitments.points()[0])
        .decompress()
        .ok_or_else(|| FeldmanError::MalformedCommitments("point 0 is invalid".to_string()))?;
    if RISTRETTO_BASEPOINT_POINT * scalar != committed {
        return Err(FeldmanError::ReconstructionMismatch);
    }
    Ok(secret)
}

/// Session-start gate: refuse to proceed unless the commitments are the ones
/// recorded on-chain and every share held satisfies them.
///
/// Call this before any hand is dealt. The transcript hash is checked first, so
/// a node handed forged commitments cannot be talked into "verifying" a forged
/// share against them.
pub fn verify_before_deal(
    commitments: &Commitments,
    ctx: &TranscriptContext,
    onchain_hash: &[u8; 32],
    shares: &[Share],
) -> Result<(), FeldmanError> {
    if &transcript_hash(commitments, ctx) != onchain_hash {
        return Err(FeldmanError::TranscriptMismatch);
    }
    for share in shares {
        verify_share(commitments, share)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng(seed: u64) -> StdRng {
        StdRng::seed_from_u64(seed)
    }

    fn dealt(threshold: u8, shares: u8, seed: u64) -> ([u8; 32], Vec<Share>, Commitments) {
        let mut rng = rng(seed);
        let secret = random_secret(&mut rng);
        let (shares, commitments) = deal(&secret, threshold, shares, &mut rng).unwrap();
        (secret, shares, commitments)
    }

    fn ctx(shares: u8) -> TranscriptContext<'static> {
        TranscriptContext {
            node_id: 3,
            public_key: "GBEXAMPLEPUBLICKEYFORTESTS",
            shares,
        }
    }

    /// A share with the same index but a different (still canonical) value.
    fn corrupted(share: &Share) -> Share {
        let value = share.scalar().unwrap() + Scalar::ONE;
        Share::new(share.index(), value.as_bytes()).unwrap()
    }

    #[test]
    fn every_honest_share_verifies() {
        for (t, n) in [(2u8, 3u8), (3, 5), (5, 5), (2, 2), (4, 9)] {
            let (_, shares, commitments) = dealt(t, n, u64::from(t) * 100 + u64::from(n));
            assert_eq!(commitments.threshold(), t);
            assert_eq!(shares.len(), n as usize);
            for share in &shares {
                verify_share(&commitments, share).unwrap();
            }
            assert!(find_invalid_shares(&commitments, &shares).is_empty());
        }
    }

    #[test]
    fn any_quorum_reconstructs_the_secret() {
        let (secret, shares, commitments) = dealt(3, 5, 1);
        for a in 0..5 {
            for b in (a + 1)..5 {
                for c in (b + 1)..5 {
                    let quorum = vec![shares[a].clone(), shares[b].clone(), shares[c].clone()];
                    assert_eq!(reconstruct(&quorum).unwrap(), secret);
                    assert_eq!(reconstruct_verified(&commitments, &quorum).unwrap(), secret);
                }
            }
        }
    }

    #[test]
    fn more_than_a_quorum_also_reconstructs() {
        let (secret, shares, commitments) = dealt(2, 4, 2);
        assert_eq!(reconstruct_verified(&commitments, &shares).unwrap(), secret);
    }

    #[test]
    fn below_threshold_is_refused_and_does_not_leak_the_secret() {
        let (secret, shares, commitments) = dealt(3, 5, 3);
        let short = vec![shares[0].clone(), shares[1].clone()];
        assert_eq!(
            reconstruct_verified(&commitments, &short),
            Err(FeldmanError::NotEnoughShares { needed: 3, got: 2 })
        );
        assert_ne!(reconstruct(&short).unwrap(), secret);
    }

    #[test]
    fn a_tampered_share_is_detected_and_attributed() {
        let (_, shares, commitments) = dealt(2, 3, 4);
        let bad = corrupted(&shares[1]);
        assert_eq!(
            verify_share(&commitments, &bad),
            Err(FeldmanError::ShareMismatch { index: 2 })
        );
        let handed_out = vec![shares[0].clone(), bad, shares[2].clone()];
        assert_eq!(find_invalid_shares(&commitments, &handed_out), vec![2]);
    }

    #[test]
    fn a_share_filed_under_the_wrong_index_is_detected() {
        let (_, shares, commitments) = dealt(2, 3, 5);
        let relabelled = Share::new(3, shares[0].as_bytes()).unwrap();
        assert_eq!(
            verify_share(&commitments, &relabelled),
            Err(FeldmanError::ShareMismatch { index: 3 })
        );
    }

    #[test]
    fn a_share_from_another_ceremony_is_detected() {
        let (_, shares_a, _) = dealt(2, 3, 6);
        let (_, _, commitments_b) = dealt(2, 3, 7);
        for share in &shares_a {
            assert!(verify_share(&commitments_b, share).is_err());
        }
    }

    #[test]
    fn reconstruct_verified_rejects_a_bad_share_before_using_it() {
        let (_, shares, commitments) = dealt(2, 3, 8);
        let quorum = vec![shares[0].clone(), corrupted(&shares[1])];
        assert_eq!(
            reconstruct_verified(&commitments, &quorum),
            Err(FeldmanError::ShareMismatch { index: 2 })
        );
    }

    #[test]
    fn commitments_with_a_swapped_constant_term_are_rejected() {
        // Replace C_0 with another ceremony's: the shares no longer satisfy the
        // commitments, so reconstruction is refused rather than returning a
        // secret that was never committed to.
        let (_, shares, commitments) = dealt(2, 3, 9);
        let (_, _, other) = dealt(2, 3, 10);
        let mut points = commitments.points().to_vec();
        points[0] = other.points()[0];
        let mixed = Commitments::from_points(points).unwrap();
        assert!(reconstruct_verified(&mixed, &shares[..2]).is_err());
    }

    #[test]
    fn rejects_bad_parameters() {
        let secret = random_secret(&mut rng(11));
        for (t, n) in [(1u8, 3u8), (0, 3), (4, 3), (2, 255)] {
            assert_eq!(
                deal(&secret, t, n, &mut rng(12)).err(),
                Some(FeldmanError::InvalidParameters {
                    threshold: t,
                    shares: n
                })
            );
        }
    }

    #[test]
    fn rejects_a_non_canonical_secret() {
        assert_eq!(
            deal(&[0xff; 32], 2, 3, &mut rng(13)).err(),
            Some(FeldmanError::SecretOutOfRange)
        );
    }

    #[test]
    fn rejects_malformed_shares() {
        assert_eq!(Share::new(0, &[1u8; 32]).err(), Some(FeldmanError::ZeroIndex));
        assert_eq!(
            Share::new(1, &[1u8; 31]).err(),
            Some(FeldmanError::MalformedShare { index: 1 })
        );
        assert_eq!(
            Share::new(2, &[0xff; 32]).err(),
            Some(FeldmanError::MalformedShare { index: 2 })
        );
    }

    #[test]
    fn rejects_duplicate_share_indices() {
        let (_, shares, _) = dealt(2, 3, 14);
        let duplicated = vec![shares[0].clone(), shares[0].clone()];
        assert_eq!(
            reconstruct(&duplicated),
            Err(FeldmanError::DuplicateIndex(1))
        );
    }

    #[test]
    fn rejects_malformed_commitments() {
        let (_, _, commitments) = dealt(2, 3, 15);
        // wrong count
        assert!(Commitments::from_points(vec![commitments.points()[0]]).is_err());
        // not a valid ristretto encoding
        assert!(
            Commitments::from_points(vec![commitments.points()[0], [0xff; 32]]).is_err()
        );
        // bad hex
        assert!(Commitments::from_hex(&["zz", "zz"]).is_err());
        // wrong length hex
        assert!(Commitments::from_hex(&["00", "00"]).is_err());
    }

    #[test]
    fn commitments_round_trip_through_hex() {
        let (_, _, commitments) = dealt(3, 5, 16);
        let restored = Commitments::from_hex(&commitments.to_hex()).unwrap();
        assert_eq!(restored, commitments);
    }

    #[test]
    fn transcript_hash_is_deterministic() {
        let (_, _, commitments) = dealt(2, 3, 17);
        assert_eq!(
            transcript_hash(&commitments, &ctx(3)),
            transcript_hash(&commitments, &ctx(3))
        );
    }

    #[test]
    fn transcript_hash_binds_every_input() {
        let (_, _, commitments) = dealt(2, 3, 18);
        let (_, _, other) = dealt(2, 3, 19);
        let base = transcript_hash(&commitments, &ctx(3));

        assert_ne!(base, transcript_hash(&other, &ctx(3)), "commitments");
        assert_ne!(base, transcript_hash(&commitments, &ctx(4)), "share count");
        assert_ne!(
            base,
            transcript_hash(
                &commitments,
                &TranscriptContext {
                    node_id: 4,
                    ..ctx(3)
                }
            ),
            "node id"
        );
        assert_ne!(
            base,
            transcript_hash(
                &commitments,
                &TranscriptContext {
                    public_key: "GANOTHERKEY",
                    ..ctx(3)
                }
            ),
            "public key"
        );

        // Reordering the same points changes the hash too.
        let mut swapped = commitments.points().to_vec();
        swapped.swap(0, 1);
        let swapped = Commitments::from_points(swapped).unwrap();
        assert_ne!(base, transcript_hash(&swapped, &ctx(3)), "point order");
    }

    #[test]
    fn session_start_check_passes_for_an_honest_ceremony() {
        let (_, shares, commitments) = dealt(2, 3, 20);
        let onchain = transcript_hash(&commitments, &ctx(3));
        verify_before_deal(&commitments, &ctx(3), &onchain, &shares).unwrap();
    }

    #[test]
    fn session_start_check_rejects_an_invalid_share() {
        let (_, shares, commitments) = dealt(2, 3, 21);
        let onchain = transcript_hash(&commitments, &ctx(3));
        let handed_out = vec![shares[0].clone(), corrupted(&shares[1])];
        assert_eq!(
            verify_before_deal(&commitments, &ctx(3), &onchain, &handed_out),
            Err(FeldmanError::ShareMismatch { index: 2 })
        );
    }

    #[test]
    fn session_start_check_rejects_commitments_that_are_not_the_onchain_ones() {
        let (_, shares, commitments) = dealt(2, 3, 22);
        let (_, _, forged) = dealt(2, 3, 23);
        let onchain = transcript_hash(&commitments, &ctx(3));
        assert_eq!(
            verify_before_deal(&forged, &ctx(3), &onchain, &shares),
            Err(FeldmanError::TranscriptMismatch)
        );
    }

    #[test]
    fn session_start_check_reports_the_transcript_before_the_shares() {
        let (_, shares, commitments) = dealt(2, 3, 24);
        let (_, _, forged) = dealt(2, 3, 25);
        let onchain = transcript_hash(&commitments, &ctx(3));
        // The share is wrong for `forged` AND the transcript is wrong: the
        // transcript failure must win, so forged commitments are never used.
        assert_eq!(
            verify_before_deal(&forged, &ctx(3), &onchain, &shares[..1]),
            Err(FeldmanError::TranscriptMismatch)
        );
    }

    #[test]
    fn derived_seed_is_stable_and_depends_on_the_secret() {
        let a = random_secret(&mut rng(26));
        let b = random_secret(&mut rng(27));
        assert_eq!(derive_seed(&a), derive_seed(&a));
        assert_ne!(derive_seed(&a), derive_seed(&b));
        assert_ne!(&derive_seed(&a), &a, "seed must not equal the secret scalar");
    }

    #[test]
    fn random_secret_is_canonical_and_non_zero() {
        let mut rng = rng(28);
        for _ in 0..32 {
            let secret = random_secret(&mut rng);
            assert_ne!(secret, [0u8; 32]);
            let parsed: Option<Scalar> = Scalar::from_canonical_bytes(secret).into();
            assert!(parsed.is_some());
        }
    }

    #[test]
    fn share_debug_output_is_redacted() {
        let (_, shares, _) = dealt(2, 3, 29);
        let rendered = format!("{:?}", shares[0]);
        assert!(rendered.contains("REDACTED"));
        assert!(!rendered.contains(&hex::encode(shares[0].as_bytes())));
    }
}
