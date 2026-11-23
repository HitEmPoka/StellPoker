//! `mpc-enroll` — MPC node key enrollment ceremony CLI (Issue #247)
//!
//! Runs the one-off ceremony that brings a new MPC node into the committee:
//!
//! 1. **Generate** the node's Stellar ed25519 keypair (the identity it signs
//!    committee transactions with, and the `member` address the
//!    `committee-registry` contract authorises).
//! 2. **Split** the node's secret into `n` Feldman-verifiable shares with
//!    threshold `t`, so no single ceremony participant — including the operator
//!    running this tool — ends up holding a recoverable copy of the node key,
//!    and every share can be checked against published commitments (#884).
//! 3. **Distribute** one share file per committee member.
//! 4. **Register** the node with the `committee-registry` contract.
//!
//! ## Why Shamir and not a DKG
//!
//! `docs/committee-setup.md` explains that coNoir REP3 needs no threshold key
//! pair: proving happens over replicated shares of the *inputs*, not under a
//! shared signing key. So the thing worth protecting here is the node's own
//! long-lived identity key, and the risk is a single operator's laptop being
//! the single point of compromise **and** the single point of loss. Splitting
//! `t`-of-`n` fixes both: fewer than `t` members learn nothing, and any `t`
//! can restore a node after a disk failure.
//!
//! ## Verifiable shares (#884)
//!
//! Plain Shamir cannot tell a holder whether their share is genuine. The split
//! here is Feldman's verifiable scheme over Ristretto255 (see
//! `mpc_node::crypto::feldman`): the ceremony shares one random scalar and
//! derives the node's ed25519 seed from it. `generate` also writes
//! `commitments.json`, the public commitments to the sharing polynomial, and a
//! SHA-256 transcript hash over them. Anyone holding a share can run `verify`
//! to confirm it lies on the committed polynomial, and `register` records the
//! transcript hash on-chain in the committee registry so nodes can check, at
//! session start, that the commitments they hold are the agreed ones.
//!
//! Ceremonies enrolled before #884 used byte-wise Shamir over GF(256). Their
//! share files carry no `scheme` field and `combine` still restores them, but
//! they have no commitments and so cannot be verified.
//!
//! ## Ceremony hygiene
//!
//! Share files are written `0600` (unix) and must be moved to their holders
//! over an out-of-band channel and then deleted from the ceremony machine.
//! This tool deliberately does not ship shares over the network: there is no
//! authenticated channel to the members at enrollment time — establishing one
//! is what enrollment is *for* — so pushing shares automatically would mean
//! trusting an unauthenticated endpoint with the material the ceremony exists
//! to protect.
//!
//! ## Usage
//!
//! ```text
//! # 1. ceremony machine (offline)
//! mpc-enroll generate --node-id 0 --threshold 2 --shares 3 \
//!     --endpoint http://mpc-node-0:8101 --region us-east \
//!     --out-dir ./ceremony
//!
//! # 2. each holder checks their own share against the public commitments
//! mpc-enroll verify --out-dir ./ceremony --share ./ceremony/share_0.json
//!
//! # 3. verify a quorum can restore the key, then destroy the plaintext seed
//! mpc-enroll combine --out-dir ./ceremony --share ./ceremony/share_0.json \
//!     --share ./ceremony/share_1.json
//!
//! # 4. register on-chain and record the transcript hash (prints the commands
//! #    unless --execute is passed)
//! mpc-enroll register --out-dir ./ceremony --registry C... \
//!     --source node0 --network testnet --stake 1000000000
//! ```

use std::fs;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use mpc_node::crypto::feldman::{self, Commitments, Share, TranscriptContext};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const MANIFEST: &str = "enrollment.json";
/// Public Feldman commitments + transcript hash (#884). Not secret.
const COMMITMENTS: &str = "commitments.json";
/// `ShareFile::scheme` of ceremonies run before verifiable sharing existed.
const LEGACY_SCHEME: &str = "gf256";

/// A value that must never be rendered by a derived/debug log statement.
/// Serialization remains available for the encrypted-at-rest ceremony file,
/// while accidental `Debug` output only exposes the type name.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct Secret<T>(T);

impl<T> Secret<T> {
    fn new(value: T) -> Self { Self(value) }
    fn expose(&self) -> &T { &self.0 }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Parser)]
#[command(
    name = "mpc-enroll",
    about = "MPC node key enrollment ceremony for Stellar Poker committees"
)]
struct Cli {
    #[command(subcommand)]
    command: Ceremony,
}

#[derive(Subcommand)]
enum Ceremony {
    /// Generate the node keypair, split it into shares, and write the manifest.
    Generate {
        #[arg(long)]
        node_id: u32,
        /// Shares required to reconstruct the seed.
        #[arg(long, default_value_t = 2)]
        threshold: u8,
        /// Total shares to hand out, one per committee member.
        #[arg(long, default_value_t = 3)]
        shares: u8,
        /// Public endpoint recorded on-chain for this node.
        #[arg(long)]
        endpoint: String,
        #[arg(long, default_value = "unknown")]
        region: String,
        #[arg(long, default_value = "./ceremony")]
        out_dir: PathBuf,
    },
    /// Check share files against the ceremony's public commitments. Reveals and
    /// reconstructs nothing, so a holder can run it on their own share.
    Verify {
        #[arg(long, default_value = "./ceremony")]
        out_dir: PathBuf,
        /// Repeat once per share file to check; none checks the commitments only.
        #[arg(long = "share")]
        shares: Vec<PathBuf>,
    },
    /// Reconstruct the seed from a quorum of shares and check it against the
    /// manifest's public key.
    Combine {
        #[arg(long, default_value = "./ceremony")]
        out_dir: PathBuf,
        /// Repeat once per share file; at least `threshold` of them.
        #[arg(long = "share", required = true)]
        shares: Vec<PathBuf>,
    },
    /// Register the enrolled node with the committee-registry contract.
    Register {
        #[arg(long, default_value = "./ceremony")]
        out_dir: PathBuf,
        /// Committee registry contract id.
        #[arg(long)]
        registry: String,
        /// Stellar CLI source account key name that pays for the transaction.
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Stake in stroops; must be at least the registry's min stake.
        #[arg(long, default_value_t = 1_000_000_000)]
        stake: i64,
        #[arg(long, default_value_t = 0)]
        fee_rate_bps: u32,
        /// Actually run the stellar CLI instead of printing the command.
        #[arg(long)]
        execute: bool,
    },
}

/// Public record of the ceremony. Contains no secret material.
#[derive(Serialize, Deserialize)]
struct Manifest {
    node_id: u32,
    /// Stellar public key (`G...`) — the committee member address.
    public_key: String,
    endpoint: String,
    region: String,
    threshold: u8,
    shares: u8,
    /// Hex SHA-256 transcript hash of the Feldman commitments; the value
    /// `register` records on-chain. Absent for ceremonies run before #884.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transcript_hash: Option<String>,
}

/// Public record of the Feldman commitments (#884). Contains no secret
/// material, so it can be published for anyone to verify a share against.
#[derive(Serialize, Deserialize)]
struct CommitmentsFile {
    scheme: String,
    node_id: u32,
    public_key: String,
    threshold: u8,
    shares: u8,
    /// Hex-encoded compressed Ristretto points `C_0 .. C_{t-1}`.
    commitments: Vec<String>,
    /// Hex SHA-256 over the parameters and commitments (the on-chain value).
    transcript_hash: String,
}

fn legacy_scheme() -> String {
    LEGACY_SCHEME.to_string()
}

/// One committee member's share. `share` alone reveals nothing about the seed.
#[derive(Serialize, Deserialize)]
struct ShareFile {
    node_id: u32,
    /// Shamir x-coordinate, 1-based; 0 is the secret itself and never issued.
    index: u8,
    threshold: u8,
    /// Hex-encoded 32-byte share.
    share: Secret<String>,
    /// Lets the holder confirm the share belongs to this node's ceremony.
    public_key: String,
    /// Sharing scheme. Files written before #884 have none and are GF(256).
    #[serde(default = "legacy_scheme")]
    scheme: String,
}

// ── GF(256) arithmetic (AES field, x^8 + x^4 + x^3 + x + 1) ─────────────────

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            product ^= a;
        }
        let high = a & 0x80;
        a <<= 1;
        if high != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    product
}

fn gf_pow(base: u8, mut exp: u32) -> u8 {
    let mut result = 1u8;
    let mut acc = base;
    while exp > 0 {
        if exp & 1 == 1 {
            result = gf_mul(result, acc);
        }
        acc = gf_mul(acc, acc);
        exp >>= 1;
    }
    result
}

/// Multiplicative inverse via Fermat: a^254 = a^-1 in GF(256).
fn gf_inv(a: u8) -> u8 {
    debug_assert!(a != 0, "GF(256) has no inverse for 0");
    gf_pow(a, 254)
}

/// Split `secret` into `n` shares, any `t` of which reconstruct it.
///
/// Only used to build legacy-format fixtures for the tests; new ceremonies use
/// `feldman::deal`.
#[cfg_attr(not(test), allow(dead_code))]
fn split(secret: &[u8], t: u8, n: u8) -> Result<Vec<(u8, Vec<u8>)>, String> {
    if t < 2 || n < t || n == u8::MAX {
        return Err(format!(
            "invalid split parameters: need 2 <= threshold ({t}) <= shares ({n}) <= 254"
        ));
    }
    let mut rng = rand::thread_rng();
    // One random polynomial per secret byte, with the byte as constant term.
    let mut coefficients = vec![0u8; secret.len() * (t as usize - 1)];
    rng.fill_bytes(&mut coefficients);

    let mut out = Vec::with_capacity(n as usize);
    for x in 1..=n {
        let mut share = Vec::with_capacity(secret.len());
        for (byte_index, byte) in secret.iter().enumerate() {
            let mut y = *byte;
            let mut x_power = 1u8;
            for degree in 0..(t as usize - 1) {
                x_power = gf_mul(x_power, x);
                y ^= gf_mul(
                    coefficients[byte_index * (t as usize - 1) + degree],
                    x_power,
                );
            }
            share.push(y);
        }
        out.push((x, share));
    }
    coefficients.zeroize();
    Ok(out)
}

/// Lagrange interpolation at x = 0.
fn combine(shares: &[(u8, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if shares.is_empty() {
        return Err("no shares supplied".to_string());
    }
    let len = shares[0].1.len();
    if shares.iter().any(|(_, s)| s.len() != len) {
        return Err("shares have differing lengths".to_string());
    }
    for (i, (x, _)) in shares.iter().enumerate() {
        if *x == 0 {
            return Err("share index 0 is not a valid share".to_string());
        }
        if shares[..i].iter().any(|(other, _)| other == x) {
            return Err(format!("duplicate share index {x}"));
        }
    }

    let mut secret = vec![0u8; len];
    for (x_i, share_i) in shares {
        let mut basis = 1u8;
        for (x_j, _) in shares {
            if x_j != x_i {
                // (0 - x_j) / (x_i - x_j); subtraction is XOR in GF(2^k).
                basis = gf_mul(basis, gf_mul(*x_j, gf_inv(*x_i ^ *x_j)));
            }
        }
        for (byte, share_byte) in secret.iter_mut().zip(share_i.iter()) {
            *byte ^= gf_mul(basis, *share_byte);
        }
    }
    Ok(secret)
}

// ── Ceremony steps ──────────────────────────────────────────────────────────

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}

fn read_manifest(out_dir: &Path) -> Result<Manifest, String> {
    let path = out_dir.join(MANIFEST);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("reading {}: {e} (run `generate` first)", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))
}

fn generate(
    node_id: u32,
    threshold: u8,
    shares: u8,
    endpoint: String,
    region: String,
    out_dir: PathBuf,
) -> Result<(), String> {
    fs::create_dir_all(&out_dir).map_err(|e| format!("creating {}: {e}", out_dir.display()))?;

    // The ceremony shares one full-entropy scalar; the node's ed25519 seed is
    // derived from it. See `mpc_node::crypto::feldman` for why the seed itself is
    // not shared.
    let mut rng = rand::rngs::OsRng;
    let mut secret = feldman::random_secret(&mut rng);
    let mut seed = feldman::derive_seed(&secret);
    let signing_key = SigningKey::from_bytes(&seed);
    seed.zeroize();
    let public_key =
        stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();

    let dealt = feldman::deal(&secret, threshold, shares, &mut rng);
    secret.zeroize();
    let (split_shares, commitments) = dealt.map_err(|e| e.to_string())?;

    // Never hand out a share the commitments would reject.
    for share in &split_shares {
        feldman::verify_share(&commitments, share).map_err(|e| e.to_string())?;
    }

    for share in &split_shares {
        let file = ShareFile {
            node_id,
            index: share.index(),
            threshold,
            share: Secret::new(hex::encode(share.as_bytes())),
            public_key: public_key.clone(),
            scheme: feldman::SCHEME.to_string(),
        };
        let path = out_dir.join(format!("share_{}.json", share.index() - 1));
        write_private(
            &path,
            &serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?,
        )?;
        println!("wrote {}", path.display());
    }

    let transcript_hash = hex::encode(feldman::transcript_hash(
        &commitments,
        &TranscriptContext {
            node_id,
            public_key: &public_key,
            shares,
        },
    ));
    let commitments_file = CommitmentsFile {
        scheme: feldman::SCHEME.to_string(),
        node_id,
        public_key: public_key.clone(),
        threshold,
        shares,
        commitments: commitments.to_hex(),
        transcript_hash: transcript_hash.clone(),
    };
    let commitments_path = out_dir.join(COMMITMENTS);
    fs::write(
        &commitments_path,
        serde_json::to_string_pretty(&commitments_file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("writing {}: {e}", commitments_path.display()))?;

    let manifest = Manifest {
        node_id,
        public_key: public_key.clone(),
        endpoint,
        region,
        threshold,
        shares,
        transcript_hash: Some(transcript_hash.clone()),
    };
    let manifest_path = out_dir.join(MANIFEST);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("writing {}: {e}", manifest_path.display()))?;

    println!("wrote {}", commitments_path.display());
    println!("wrote {}", manifest_path.display());
    println!("\nnode {node_id} public key: {public_key}");
    println!("commitment transcript hash: {transcript_hash}");
    println!(
        "{threshold}-of-{shares} shares written. Hand each share_N.json to one committee member \
         out-of-band, then delete this directory's share files from the ceremony machine.\n\
         Publish {COMMITMENTS} so every holder can run `mpc-enroll verify` on their share; \
         `register` records the transcript hash on-chain.\n\
         The node key exists only as shares — `mpc-enroll combine` restores it."
    );
    Ok(())
}

/// Loads and cross-checks the public commitments. `Ok(None)` means a ceremony
/// from before #884 that has none.
fn load_transcript(
    out_dir: &Path,
    manifest: &Manifest,
) -> Result<Option<(Commitments, [u8; 32])>, String> {
    let path = out_dir.join(COMMITMENTS);
    if !path.exists() {
        return match manifest.transcript_hash {
            Some(_) => Err(format!(
                "{MANIFEST} records a commitment transcript hash but {} is missing",
                path.display()
            )),
            None => Ok(None),
        };
    }

    let raw = fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let file: CommitmentsFile =
        serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if file.scheme != feldman::SCHEME {
        return Err(format!(
            "{} uses unsupported scheme {:?} (expected {:?})",
            path.display(),
            file.scheme,
            feldman::SCHEME
        ));
    }
    if file.public_key != manifest.public_key || file.node_id != manifest.node_id {
        return Err(format!(
            "{} belongs to a different ceremony ({})",
            path.display(),
            file.public_key
        ));
    }

    let commitments = Commitments::from_hex(&file.commitments).map_err(|e| e.to_string())?;
    if commitments.threshold() != manifest.threshold || file.shares != manifest.shares {
        return Err(format!(
            "{} disagrees with {MANIFEST} about the threshold or share count",
            path.display()
        ));
    }

    let hash = feldman::transcript_hash(
        &commitments,
        &TranscriptContext {
            node_id: manifest.node_id,
            public_key: &manifest.public_key,
            shares: manifest.shares,
        },
    );
    let hash_hex = hex::encode(hash);
    if file.transcript_hash != hash_hex {
        return Err(format!(
            "{} has been altered: its commitments hash to {hash_hex}, not {}",
            path.display(),
            file.transcript_hash
        ));
    }
    if manifest.transcript_hash.as_deref().is_some_and(|h| h != hash_hex) {
        return Err(format!(
            "{} does not match the transcript hash recorded in {MANIFEST}",
            path.display()
        ));
    }
    Ok(Some((commitments, hash)))
}

/// Reads one share file and checks it belongs to this ceremony.
fn read_share_file(path: &Path, manifest: &Manifest) -> Result<(ShareFile, Vec<u8>), String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let file: ShareFile =
        serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if file.public_key != manifest.public_key {
        return Err(format!(
            "{} belongs to a different ceremony ({})",
            path.display(),
            file.public_key
        ));
    }
    let bytes = hex::decode(file.share.expose())
        .map_err(|e| format!("bad share hex in {}: {e}", path.display()))?;
    Ok((file, bytes))
}

fn verify_cmd(out_dir: PathBuf, share_paths: Vec<PathBuf>) -> Result<(), String> {
    let manifest = read_manifest(&out_dir)?;
    let (commitments, hash) = load_transcript(&out_dir, &manifest)?.ok_or_else(|| {
        "this ceremony has no commitments (it predates verifiable sharing, #884), \
         so its shares cannot be verified"
            .to_string()
    })?;
    println!(
        "commitments OK: {}-of-{} for {}, transcript hash {}",
        commitments.threshold(),
        manifest.shares,
        manifest.public_key,
        hex::encode(hash)
    );

    let mut invalid = Vec::new();
    for path in &share_paths {
        let (file, bytes) = read_share_file(path, &manifest)?;
        if file.scheme != feldman::SCHEME {
            return Err(format!(
                "{} is a {:?} share, which has no commitments to check against",
                path.display(),
                file.scheme
            ));
        }
        let outcome = Share::new(file.index, &bytes)
            .and_then(|share| feldman::verify_share(&commitments, &share));
        match outcome {
            Ok(()) => println!("share {}: OK ({})", file.index, path.display()),
            Err(e) => {
                println!("share {}: INVALID ({e})", file.index);
                invalid.push(file.index);
            }
        }
    }

    if invalid.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} share(s) do not match the published commitments: index {:?}",
            invalid.len(),
            invalid
        ))
    }
}

fn combine_cmd(out_dir: PathBuf, share_paths: Vec<PathBuf>) -> Result<(), String> {
    let manifest = read_manifest(&out_dir)?;
    let transcript = load_transcript(&out_dir, &manifest)?;

    let mut legacy: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut verifiable: Vec<Share> = Vec::new();
    for path in &share_paths {
        let (file, bytes) = read_share_file(path, &manifest)?;
        match file.scheme.as_str() {
            LEGACY_SCHEME => legacy.push((file.index, bytes)),
            feldman::SCHEME => {
                let share = Share::new(file.index, &bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                // Refuse a bad share by name instead of reconstructing garbage.
                if let Some((commitments, _)) = &transcript {
                    feldman::verify_share(commitments, &share)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                }
                verifiable.push(share);
            }
            other => {
                return Err(format!(
                    "{} uses unknown share scheme {other:?}",
                    path.display()
                ))
            }
        }
    }
    if !legacy.is_empty() && !verifiable.is_empty() {
        return Err("share files come from different sharing schemes".to_string());
    }
    let supplied = legacy.len() + verifiable.len();
    if supplied < manifest.threshold as usize {
        return Err(format!(
            "need at least {} shares, got {supplied}",
            manifest.threshold
        ));
    }

    let mut seed = if verifiable.is_empty() {
        combine(&legacy)?
    } else {
        let rebuilt = match &transcript {
            Some((commitments, _)) => feldman::reconstruct_verified(commitments, &verifiable),
            None => feldman::reconstruct(&verifiable),
        };
        let mut secret = rebuilt.map_err(|e| e.to_string())?;
        let derived = feldman::derive_seed(&secret);
        secret.zeroize();
        derived.to_vec()
    };
    let mut seed_array: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| format!("reconstructed seed is {} bytes, expected 32", seed.len()))?;
    let recovered = stellar_strkey::ed25519::PublicKey(
        SigningKey::from_bytes(&seed_array)
            .verifying_key()
            .to_bytes(),
    )
    .to_string();
    let secret = stellar_strkey::ed25519::PrivateKey(seed_array).to_string();
    seed.zeroize();
    seed_array.zeroize();

    if recovered != manifest.public_key {
        return Err(format!(
            "reconstruction failed: got {recovered}, manifest says {}",
            manifest.public_key
        ));
    }
    println!("reconstruction OK for {recovered}");
    println!("secret key: {secret}");
    Ok(())
}

/// Arguments for `stellar contract invoke` against the registry.
fn stellar_invoke(
    registry: &str,
    source: &str,
    network: &str,
    function: &str,
    params: &[(&str, String)],
) -> Vec<String> {
    let mut args: Vec<String> = [
        "contract", "invoke", "--id", registry, "--source", source, "--network", network, "--",
        function,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (name, value) in params {
        args.push(format!("--{name}"));
        args.push(value.clone());
    }
    args
}

fn run_stellar(args: &[String]) -> Result<(), String> {
    let status = Command::new("stellar")
        .args(args)
        .status()
        .map_err(|e| format!("running stellar CLI: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("stellar CLI exited with {status}"))
    }
}

#[allow(clippy::too_many_arguments)]
fn register(
    out_dir: PathBuf,
    registry: String,
    source: String,
    network: String,
    stake: i64,
    fee_rate_bps: u32,
    execute: bool,
) -> Result<(), String> {
    let manifest = read_manifest(&out_dir)?;
    // Refuse to register a node whose commitments have been tampered with.
    let transcript = load_transcript(&out_dir, &manifest)?;

    let register_args = stellar_invoke(
        &registry,
        &source,
        &network,
        "register_member",
        &[
            ("member", manifest.public_key.clone()),
            ("stake", stake.to_string()),
            ("endpoint", manifest.endpoint.clone()),
            ("region", manifest.region.clone()),
            ("fee_rate_bps", fee_rate_bps.to_string()),
        ],
    );
    // Recorded on-chain so the commitment transcript is queryable and nodes can
    // check it at session start (#884). Must come after `register_member`.
    let commitment_args = transcript.as_ref().map(|(_, hash)| {
        stellar_invoke(
            &registry,
            &source,
            &network,
            "set_commitment_hash",
            &[
                ("member", manifest.public_key.clone()),
                ("hash", hex::encode(hash)),
            ],
        )
    });

    if !execute {
        println!("stellar {}", register_args.join(" "));
        match &commitment_args {
            Some(args) => println!("stellar {}", args.join(" ")),
            None => println!(
                "(no commitment transcript: this ceremony predates verifiable sharing)"
            ),
        }
        println!("\n(dry run — pass --execute to submit)");
        return Ok(());
    }

    run_stellar(&register_args)?;
    println!(
        "registered {} with the committee registry",
        manifest.public_key
    );
    if let Some(args) = &commitment_args {
        run_stellar(args)?;
        println!("recorded the commitment transcript hash on-chain");
    }
    Ok(())
}

fn main() {
    let result = match Cli::parse().command {
        Ceremony::Generate {
            node_id,
            threshold,
            shares,
            endpoint,
            region,
            out_dir,
        } => generate(node_id, threshold, shares, endpoint, region, out_dir),
        Ceremony::Verify { out_dir, shares } => verify_cmd(out_dir, shares),
        Ceremony::Combine { out_dir, shares } => combine_cmd(out_dir, shares),
        Ceremony::Register {
            out_dir,
            registry,
            source,
            network,
            stake,
            fee_rate_bps,
            execute,
        } => register(
            out_dir,
            registry,
            source,
            network,
            stake,
            fee_rate_bps,
            execute,
        ),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted_but_serialization_is_preserved() {
        let secret = Secret::new("card-and-share-material".to_string());
        assert_eq!(format!("{:?}", secret), "[REDACTED]");
        let encoded = serde_json::to_string(&secret).unwrap();
        assert_eq!(encoded, "\"card-and-share-material\"");
    }

    #[test]
    fn gf_inverse_round_trips() {
        for a in 1..=255u8 {
            assert_eq!(gf_mul(a, gf_inv(a)), 1, "inverse failed for {a}");
        }
    }

    #[test]
    fn any_quorum_reconstructs() {
        let secret: Vec<u8> = (0..32).collect();
        let shares = split(&secret, 2, 3).unwrap();
        for i in 0..3 {
            for j in (i + 1)..3 {
                let quorum = vec![shares[i].clone(), shares[j].clone()];
                assert_eq!(combine(&quorum).unwrap(), secret);
            }
        }
    }

    #[test]
    fn all_shares_reconstruct_at_higher_threshold() {
        let secret = vec![0xde, 0xad, 0xbe, 0xef, 0x00, 0xff];
        let shares = split(&secret, 4, 5).unwrap();
        assert_eq!(combine(&shares).unwrap(), secret);
    }

    #[test]
    fn below_threshold_does_not_recover_the_secret() {
        let secret: Vec<u8> = vec![7; 32];
        let shares = split(&secret, 3, 5).unwrap();
        let short = vec![shares[0].clone(), shares[1].clone()];
        assert_ne!(combine(&short).unwrap(), secret);
    }

    #[test]
    fn shares_do_not_leak_the_secret_verbatim() {
        let secret: Vec<u8> = vec![0xab; 32];
        for (_, share) in split(&secret, 2, 3).unwrap() {
            assert_ne!(share, secret);
        }
    }

    #[test]
    fn rejects_bad_parameters() {
        assert!(split(&[1, 2, 3], 1, 3).is_err());
        assert!(split(&[1, 2, 3], 4, 3).is_err());
    }

    #[test]
    fn rejects_duplicate_share_indices() {
        let shares = split(&[1, 2, 3, 4], 2, 3).unwrap();
        let duplicated = vec![shares[0].clone(), shares[0].clone()];
        assert!(combine(&duplicated).is_err());
    }

    #[test]
    fn seed_round_trips_through_a_split() {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let seed = signing_key.to_bytes();
        let shares = split(&seed, 2, 3).unwrap();
        let recovered = combine(&shares[..2]).unwrap();
        let recovered: [u8; 32] = recovered.try_into().unwrap();
        assert_eq!(
            SigningKey::from_bytes(&recovered).verifying_key(),
            signing_key.verifying_key()
        );
    }

    // ── Feldman verifiable shares (#884) ────────────────────────────────────

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn write_json(path: &Path, value: &serde_json::Value) {
        fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn share_path(dir: &Path, n: usize) -> PathBuf {
        dir.join(format!("share_{n}.json"))
    }

    fn ceremony(threshold: u8, shares: u8) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        generate(
            0,
            threshold,
            shares,
            "http://mpc-node-0:8101".to_string(),
            "us-east".to_string(),
            dir.path().to_path_buf(),
        )
        .unwrap();
        dir
    }

    /// Overwrites the value held in `share_{n}.json`, leaving its index alone.
    fn set_share_value(dir: &Path, n: usize, value_hex: &str) {
        let path = share_path(dir, n);
        let mut json = read_json(&path);
        json["share"] = serde_json::Value::String(value_hex.to_string());
        write_json(&path, &json);
    }

    fn share_value(dir: &Path, n: usize) -> String {
        read_json(&share_path(dir, n))["share"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// A ceremony in the pre-#884 format: GF(256) shares, no commitments.
    fn legacy_ceremony() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let public_key =
            stellar_strkey::ed25519::PublicKey(key.verifying_key().to_bytes()).to_string();
        for (index, share) in split(&key.to_bytes(), 2, 3).unwrap() {
            write_json(
                &share_path(dir.path(), index as usize - 1),
                &serde_json::json!({
                    "node_id": 0,
                    "index": index,
                    "threshold": 2,
                    "share": hex::encode(share),
                    "public_key": public_key,
                }),
            );
        }
        write_json(
            &dir.path().join(MANIFEST),
            &serde_json::json!({
                "node_id": 0,
                "public_key": public_key,
                "endpoint": "http://mpc-node-0:8101",
                "region": "us-east",
                "threshold": 2,
                "shares": 3,
            }),
        );
        dir
    }

    #[test]
    fn generate_writes_commitments_and_records_the_transcript_hash() {
        let dir = ceremony(2, 3);
        let manifest = read_manifest(dir.path()).unwrap();
        let commitments = read_json(&dir.path().join(COMMITMENTS));

        assert_eq!(commitments["scheme"], feldman::SCHEME);
        assert_eq!(commitments["commitments"].as_array().unwrap().len(), 2);
        assert_eq!(
            manifest.transcript_hash.as_deref(),
            commitments["transcript_hash"].as_str()
        );
        assert!(load_transcript(dir.path(), &manifest).unwrap().is_some());
    }

    #[test]
    fn generated_share_files_name_the_scheme() {
        let dir = ceremony(2, 3);
        for n in 0..3 {
            assert_eq!(read_json(&share_path(dir.path(), n))["scheme"], feldman::SCHEME);
        }
    }

    #[test]
    fn every_generated_share_verifies_against_the_commitments() {
        let dir = ceremony(3, 5);
        let paths: Vec<PathBuf> = (0..5).map(|n| share_path(dir.path(), n)).collect();
        verify_cmd(dir.path().to_path_buf(), paths).unwrap();
    }

    #[test]
    fn verify_without_shares_checks_the_commitments_only() {
        let dir = ceremony(2, 3);
        verify_cmd(dir.path().to_path_buf(), vec![]).unwrap();
    }

    #[test]
    fn verify_names_the_invalid_share() {
        let dir = ceremony(2, 3);
        // share_1.json holds index 2; give it share_0's value.
        let stolen = share_value(dir.path(), 0);
        set_share_value(dir.path(), 1, &stolen);

        let paths: Vec<PathBuf> = (0..3).map(|n| share_path(dir.path(), n)).collect();
        let err = verify_cmd(dir.path().to_path_buf(), paths).unwrap_err();
        assert!(err.contains("do not match the published commitments"), "{err}");
        assert!(err.contains("[2]"), "{err}");
    }

    #[test]
    fn combine_restores_the_node_key_from_any_quorum() {
        let dir = ceremony(2, 3);
        let manifest = read_manifest(dir.path()).unwrap();
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let shares = vec![share_path(dir.path(), a), share_path(dir.path(), b)];
            combine_cmd(dir.path().to_path_buf(), shares).unwrap();
        }
        assert!(manifest.public_key.starts_with('G'));
    }

    #[test]
    fn combine_refuses_a_tampered_share_and_names_it() {
        let dir = ceremony(2, 3);
        let stolen = share_value(dir.path(), 0);
        set_share_value(dir.path(), 1, &stolen);

        let shares = vec![share_path(dir.path(), 0), share_path(dir.path(), 1)];
        let err = combine_cmd(dir.path().to_path_buf(), shares).unwrap_err();
        assert!(err.contains("does not match the published commitments"), "{err}");
        assert!(err.contains("share 2"), "{err}");
    }

    #[test]
    fn combine_rejects_too_few_shares() {
        let dir = ceremony(3, 5);
        let err = combine_cmd(dir.path().to_path_buf(), vec![share_path(dir.path(), 0)])
            .unwrap_err();
        assert!(err.contains("need at least 3 shares"), "{err}");
    }

    #[test]
    fn altered_commitments_are_rejected() {
        let dir = ceremony(2, 3);
        let path = dir.path().join(COMMITMENTS);
        let mut json = read_json(&path);
        let points = json["commitments"].as_array_mut().unwrap();
        points.swap(0, 1);
        write_json(&path, &json);

        let manifest = read_manifest(dir.path()).unwrap();
        let err = load_transcript(dir.path(), &manifest).err().unwrap();
        assert!(err.contains("altered"), "{err}");
    }

    #[test]
    fn a_missing_commitments_file_is_an_error_when_a_hash_was_recorded() {
        let dir = ceremony(2, 3);
        fs::remove_file(dir.path().join(COMMITMENTS)).unwrap();
        let manifest = read_manifest(dir.path()).unwrap();
        let err = load_transcript(dir.path(), &manifest).err().unwrap();
        assert!(err.contains("is missing"), "{err}");
    }

    #[test]
    fn a_share_from_another_ceremony_is_rejected() {
        let a = ceremony(2, 3);
        let b = ceremony(2, 3);
        let shares = vec![share_path(a.path(), 0), share_path(b.path(), 1)];
        let err = combine_cmd(a.path().to_path_buf(), shares).unwrap_err();
        assert!(err.contains("different ceremony"), "{err}");
    }

    #[test]
    fn generate_rejects_invalid_parameters() {
        let dir = tempfile::tempdir().unwrap();
        for (t, n) in [(1u8, 3u8), (4, 3)] {
            assert!(generate(
                0,
                t,
                n,
                "http://x".to_string(),
                "us".to_string(),
                dir.path().to_path_buf()
            )
            .is_err());
        }
        assert!(!dir.path().join(MANIFEST).exists());
    }

    #[test]
    fn legacy_share_files_default_to_the_gf256_scheme() {
        let file: ShareFile = serde_json::from_str(
            r#"{"node_id":0,"index":1,"threshold":2,"share":"00","public_key":"G"}"#,
        )
        .unwrap();
        assert_eq!(file.scheme, LEGACY_SCHEME);
    }

    #[test]
    fn a_legacy_ceremony_still_combines_but_cannot_be_verified() {
        let dir = legacy_ceremony();
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(manifest.transcript_hash.is_none());
        assert!(load_transcript(dir.path(), &manifest).unwrap().is_none());

        let shares = vec![share_path(dir.path(), 0), share_path(dir.path(), 2)];
        combine_cmd(dir.path().to_path_buf(), shares).unwrap();

        let err = verify_cmd(dir.path().to_path_buf(), vec![]).unwrap_err();
        assert!(err.contains("predates verifiable sharing"), "{err}");
    }

    #[test]
    fn mixing_legacy_and_verifiable_shares_is_rejected() {
        let legacy = legacy_ceremony();
        let modern = ceremony(2, 3);
        // Same directory layout, but make the modern share claim the legacy
        // ceremony's public key so only the scheme check can stop it.
        let legacy_key = read_manifest(legacy.path()).unwrap().public_key;
        let path = share_path(modern.path(), 1);
        let mut json = read_json(&path);
        json["public_key"] = serde_json::Value::String(legacy_key);
        write_json(&path, &json);
        fs::copy(&path, share_path(legacy.path(), 1)).unwrap();

        let shares = vec![share_path(legacy.path(), 0), share_path(legacy.path(), 1)];
        let err = combine_cmd(legacy.path().to_path_buf(), shares).unwrap_err();
        assert!(err.contains("different sharing schemes"), "{err}");
    }

    #[test]
    fn stellar_invoke_builds_the_cli_arguments() {
        let args = stellar_invoke(
            "CREG",
            "node0",
            "testnet",
            "set_commitment_hash",
            &[("member", "GMEMBER".to_string()), ("hash", "ab".to_string())],
        );
        assert_eq!(
            args,
            [
                "contract",
                "invoke",
                "--id",
                "CREG",
                "--source",
                "node0",
                "--network",
                "testnet",
                "--",
                "set_commitment_hash",
                "--member",
                "GMEMBER",
                "--hash",
                "ab"
            ]
        );
    }

    #[test]
    fn register_dry_run_succeeds_and_refuses_altered_commitments() {
        let dir = ceremony(2, 3);
        let dry_run = |dir: &Path| {
            register(
                dir.to_path_buf(),
                "CREG".to_string(),
                "node0".to_string(),
                "testnet".to_string(),
                1_000_000_000,
                0,
                false,
            )
        };
        dry_run(dir.path()).unwrap();

        let path = dir.path().join(COMMITMENTS);
        let mut json = read_json(&path);
        json["commitments"].as_array_mut().unwrap().swap(0, 1);
        write_json(&path, &json);
        let err = dry_run(dir.path()).unwrap_err();
        assert!(err.contains("altered"), "{err}");
    }
}
