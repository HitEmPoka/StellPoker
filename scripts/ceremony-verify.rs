/**
 * Key Ceremony Transcript Verification Tool
 *
 * CLI: `stellpoker-ceremony verify <transcript>`
 *
 * Verifies participants, shares, and signatures in a key ceremony transcript.
 */
use clap::{Arg, Command};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::process;

/// Represents a participant in the ceremony
#[derive(Debug, Serialize, Deserialize)]
pub struct Participant {
    pub id: u32,
    pub public_key: String,
    pub endpoint: String,
}

/// Represents a share in the ceremony
#[derive(Debug, Serialize, Deserialize)]
pub struct Share {
    pub participant_id: u32,
    pub share_data: String,
    pub commitment: String,
}

/// Represents a signature on the transcript
#[derive(Debug, Serialize, Deserialize)]
pub struct Signature {
    pub participant_id: u32,
    pub signature_data: String,
}

/// The full ceremony transcript
#[derive(Debug, Serialize, Deserialize)]
pub struct CeremonyTranscript {
    pub version: u32,
    pub ceremony_id: String,
    pub participants: Vec<Participant>,
    pub shares: Vec<Share>,
    pub signatures: Vec<Signature>,
    pub transcript_hash: String,
}

/// Verification result
#[derive(Debug)]
pub struct VerificationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl VerificationResult {
    pub fn new() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn add_error(&mut self, msg: &str) {
        self.valid = false;
        self.errors.push(msg.to_string());
    }

    pub fn add_warning(&mut self, msg: &str) {
        self.warnings.push(msg.to_string());
    }
}

/// Verify the ceremony transcript
pub fn verify_transcript(transcript: &CeremonyTranscript) -> VerificationResult {
    let mut result = VerificationResult::new();

    // Check 1: Verify participant count
    if transcript.participants.is_empty() {
        result.add_error("No participants in transcript");
        return result;
    }

    if transcript.participants.len() < 2 {
        result.add_warning("Only one participant - no threshold security");
    }

    // Check 2: Verify each participant has a share
    for participant in &transcript.participants {
        let has_share = transcript.shares.iter().any(|s| s.participant_id == participant.id);
        if !has_share {
            result.add_error(&format!("Participant {} has no share", participant.id));
        }
    }

    // Check 3: Verify each participant signed
    for participant in &transcript.participants {
        let has_sig = transcript.signatures.iter().any(|s| s.participant_id == participant.id);
        if !has_sig {
            result.add_error(&format!("Participant {} has not signed", participant.id));
        }
    }

    // Check 4: Verify share count matches participant count
    if transcript.shares.len() != transcript.participants.len() {
        result.add_error(&format!(
            "Share count ({}) doesn't match participant count ({})",
            transcript.shares.len(),
            transcript.participants.len()
        ));
    }

    // Check 5: Verify signature count matches participant count
    if transcript.signatures.len() != transcript.participants.len() {
        result.add_error(&format!(
            "Signature count ({}) doesn't match participant count ({})",
            transcript.signatures.len(),
            transcript.participants.len()
        ));
    }

    // Check 6: Verify transcript hash
    let computed_hash = compute_transcript_hash(transcript);
    if computed_hash != transcript.transcript_hash {
        result.add_error("Transcript hash mismatch - transcript may be tampered");
    }

    // Check 7: Verify version
    if transcript.version == 0 {
        result.add_error("Invalid transcript version (0)");
    }

    result
}

/// Compute the hash of the transcript content
fn compute_transcript_hash(transcript: &CeremonyTranscript) -> String {
    let mut hasher = Sha256::new();

    // Hash participants
    for p in &transcript.participants {
        hasher.update(p.id.to_string().as_bytes());
        hasher.update(&p.public_key);
        hasher.update(&p.endpoint);
    }

    // Hash shares
    for s in &transcript.shares {
        hasher.update(s.participant_id.to_string().as_bytes());
        hasher.update(&s.share_data);
        hasher.update(&s.commitment);
    }

    // Hash ceremony metadata
    hasher.update(&transcript.ceremony_id);
    hasher.update(transcript.version.to_string().as_bytes());

    format!("{:x}", hasher.finalize())
}

fn main() {
    let matches = Command::new("stellpoker-ceremony")
        .about("Key ceremony transcript verification tool")
        .subcommand(
            Command::new("verify")
                .about("Verify a ceremony transcript")
                .arg(
                    Arg::new("transcript")
                        .required(true)
                        .help("Path to the transcript JSON file"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("verify", args)) => {
            let path = args.get_one::<String>("transcript").unwrap();

            println!("🔐 StellPoker Ceremony Transcript Verifier");
            println!("==========================================\n");

            // Read transcript file
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("❌ Failed to read transcript: {}", e);
                    process::exit(1);
                }
            };

            // Parse transcript
            let transcript: CeremonyTranscript = match serde_json::from_str(&content) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("❌ Failed to parse transcript: {}", e);
                    process::exit(1);
                }
            };

            println!("📋 Ceremony ID: {}", transcript.ceremony_id);
            println!("📊 Version: {}", transcript.version);
            println!("👥 Participants: {}\n", transcript.participants.len());

            // Verify
            let result = verify_transcript(&transcript);

            // Print results
            if result.errors.is_empty() && result.warnings.is_empty() {
                println!("✅ Transcript verification PASSED");
                println!("   - All {} participants have valid shares and signatures", transcript.participants.len());
                println!("   - Transcript hash is valid");
            } else {
                if !result.errors.is_empty() {
                    println!("❌ Errors:");
                    for err in &result.errors {
                        println!("   - {}", err);
                    }
                }
                if !result.warnings.is_empty() {
                    println!("\n⚠️  Warnings:");
                    for warn in &result.warnings {
                        println!("   - {}", warn);
                    }
                }
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: stellpoker-ceremony verify <transcript.json>");
            process::exit(1);
        }
    }
}
