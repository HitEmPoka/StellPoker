//! `mpc-enroll` — MPC node key enrollment ceremony CLI (Issue #247)
//!
//! Runs the one-off ceremony that brings a new MPC node into the committee:
//!
//! 1. **Generate** the node's Stellar ed25519 keypair (the identity it signs
//!    committee transactions with, and the `member` address the
//!    `committee-registry` contract authorises).
//! 2. **Split** the secret seed into `n` Shamir shares with threshold `t`, so
//!    no single ceremony participant — including the operator running this
//!    tool — ends up holding a recoverable copy of the node key.
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
//! The split is over GF(256) byte-wise, which is the standard construction for
//! sharing a fixed-size secret and keeps each share the same 32 bytes as the
//! seed.
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
//! # 2. verify a quorum can restore the key, then destroy the plaintext seed
//! mpc-enroll combine --out-dir ./ceremony --share ./ceremony/share_0.json \
//!     --share ./ceremony/share_1.json
//!
//! # 3. register on-chain (prints the command unless --execute is passed)
//! mpc-enroll register --out-dir ./ceremony --registry C... \
//!     --source node0 --network testnet --stake 1000000000
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const MANIFEST: &str = "enrollment.json";

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
}

/// One committee member's share. `share` alone reveals nothing about the seed.
#[derive(Serialize, Deserialize)]
struct ShareFile {
    node_id: u32,
    /// Shamir x-coordinate, 1-based; 0 is the secret itself and never issued.
    index: u8,
    threshold: u8,
    /// Hex-encoded 32-byte share.
    share: String,
    /// Lets the holder confirm the share belongs to this node's ceremony.
    public_key: String,
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

    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let mut seed = signing_key.to_bytes();
    let public_key =
        stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();

    let split_shares = split(&seed, threshold, shares)?;
    seed.zeroize();

    for (index, share) in &split_shares {
        let file = ShareFile {
            node_id,
            index: *index,
            threshold,
            share: hex::encode(share),
            public_key: public_key.clone(),
        };
        let path = out_dir.join(format!("share_{}.json", *index - 1));
        write_private(
            &path,
            &serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?,
        )?;
        println!("wrote {}", path.display());
    }

    let manifest = Manifest {
        node_id,
        public_key: public_key.clone(),
        endpoint,
        region,
        threshold,
        shares,
    };
    let manifest_path = out_dir.join(MANIFEST);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("writing {}: {e}", manifest_path.display()))?;

    println!("wrote {}", manifest_path.display());
    println!("\nnode {node_id} public key: {public_key}");
    println!(
        "{threshold}-of-{shares} shares written. Hand each share_N.json to one committee member \
         out-of-band, then delete this directory's share files from the ceremony machine.\n\
         The node key exists only as shares — `mpc-enroll combine` restores it."
    );
    Ok(())
}

fn combine_cmd(out_dir: PathBuf, share_paths: Vec<PathBuf>) -> Result<(), String> {
    let manifest = read_manifest(&out_dir)?;

    let mut collected = Vec::new();
    for path in &share_paths {
        let raw =
            fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let file: ShareFile =
            serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;
        if file.public_key != manifest.public_key {
            return Err(format!(
                "{} belongs to a different ceremony ({})",
                path.display(),
                file.public_key
            ));
        }
        let bytes = hex::decode(&file.share)
            .map_err(|e| format!("bad share hex in {}: {e}", path.display()))?;
        collected.push((file.index, bytes));
    }
    if collected.len() < manifest.threshold as usize {
        return Err(format!(
            "need at least {} shares, got {}",
            manifest.threshold,
            collected.len()
        ));
    }

    let mut seed = combine(&collected)?;
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
    let args = vec![
        "contract".to_string(),
        "invoke".to_string(),
        "--id".to_string(),
        registry,
        "--source".to_string(),
        source,
        "--network".to_string(),
        network,
        "--".to_string(),
        "register_member".to_string(),
        "--member".to_string(),
        manifest.public_key.clone(),
        "--stake".to_string(),
        stake.to_string(),
        "--endpoint".to_string(),
        manifest.endpoint.clone(),
        "--region".to_string(),
        manifest.region.clone(),
        "--fee_rate_bps".to_string(),
        fee_rate_bps.to_string(),
    ];

    if !execute {
        println!("stellar {}", args.join(" "));
        println!("\n(dry run — pass --execute to submit)");
        return Ok(());
    }

    let status = Command::new("stellar")
        .args(&args)
        .status()
        .map_err(|e| format!("running stellar CLI: {e}"))?;
    if !status.success() {
        return Err(format!("stellar CLI exited with {status}"));
    }
    println!(
        "registered {} with the committee registry",
        manifest.public_key
    );
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
}
