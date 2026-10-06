//! # Program Constants
//!
//! Defines all constant values used throughout the giveaways program
//! including account seeds, size limits, and default values.

// Branding and requirements moved off-chain

/// Maximum length for participant proof text (UTF-8 bytes)
pub const MAX_PROOF_TEXT_LEN: usize = 256;

/// Maximum length for a lucky phrase in proof-of-chance (UTF-8 bytes)
pub const MAX_LUCKY_WORDS_LEN: usize = 32;

/// Maximum length for salt in proof-of-chance (bytes)
pub const MAX_SALT_LEN: usize = 16;

/// Maximum length for reveal plaintext (lucky_words + 0x1f + salt)
pub const MAX_REVEAL_PLAINTEXT_LEN: usize = MAX_LUCKY_WORDS_LEN + 1 + MAX_SALT_LEN;

/// Separator between the lucky phrase and salt in proof-of-chance reveal plaintext.
pub const REVEAL_SEPARATOR: u8 = 0x1f;

/// Maximum service fee in basis points (99.99%)
pub const MAX_SERVICE_FEE_BPS: u16 = 9999;

/// Minimum number of winners allowed
pub const MIN_WINNERS: u32 = 1;

/// Maximum number of winners allowed per giveaway
pub const MAX_WINNERS: u32 = 1000;

/// Bound the resumable settlement workload for one giveaway.
pub const MAX_PARTICIPANTS: u64 = 4_096;

/// Minimum amount a finalized winner must receive.
pub const MIN_WINNER_PAYOUT_LAMPORTS: u64 = 1;

/// Minimum active duration in seconds (1 hour)
pub const MIN_ACTIVE_DURATION_SECS: u32 = 3600;

/// Maximum active duration in seconds (30 days)
pub const MAX_ACTIVE_DURATION_SECS: u32 = 30 * 24 * 3600;

/// Minimum upload/attestation duration in seconds (1 hour)
pub const MIN_UPLOAD_DURATION_SECS: u32 = 3600;

/// Maximum upload/attestation duration in seconds (7 days)
pub const MAX_UPLOAD_DURATION_SECS: u32 = 7 * 24 * 3600;

/// Default active duration in seconds (24 hours)
pub const DEFAULT_ACTIVE_DURATION_SECS: u32 = 24 * 3600;

/// Default upload/attestation duration in seconds (24 hours)
pub const DEFAULT_UPLOAD_DURATION_SECS: u32 = 24 * 3600;

/// Default remediation/challenge window after upload deadline when accepted
/// attested reveals have not all been included.
pub const DEFAULT_REMEDIATION_WINDOW_SECS: i64 = 30 * 60;

/// Default service fee in basis points (5%)
pub const DEFAULT_SERVICE_FEE_BPS: u16 = 500;

/// Minimum lamports for rent exemption buffer
pub const MIN_RENT_EXEMPTION: u64 = 1_000_000; // ~0.001 SOL

/// Maximum participants per reveal upload batch
pub const MAX_REVEALS_PER_BATCH: usize = 50;

/// Maximum winners per settlement batch
pub const MAX_WINNERS_PER_BATCH: usize = 50;

// Proof-of-chance domains
pub const GIVEAWAY_ATTESTATION_DOMAIN_V1: &[u8] = b"GIVEAWAY_ATTEST_V1";
pub const GIVEAWAY_REVEAL_DOMAIN_V1: &[u8] = b"GIVEAWAY_REVEAL_V1";
pub const GIVEAWAY_SEED_DOMAIN_V1: &[u8] = b"GIVEAWAY_SEED_V1";
pub const GIVEAWAY_DRAW_DOMAIN_V1: &[u8] = b"GIVEAWAY_DRAW_V1";
pub const GIVEAWAY_RULE_VERSION_V1: &str = "giveaway-poc-v1";
pub const GIVEAWAY_FINALIZATION_SEED_DOMAIN_V2: &[u8] = b"GIVEAWAY_FINALIZE_SEED_V2";
pub const GIVEAWAY_FINALIZATION_SEED_DOMAIN_V3: &[u8] = b"GIVEAWAY_FINALIZE_SEED_V3";
pub const GIVEAWAY_RANK_DOMAIN_V2: &[u8] = b"GIVEAWAY_RANK_V2";
pub const GIVEAWAY_RULE_VERSION_V2: &str = "giveaway-poc-radix-threshold-v2";
pub const GIVEAWAY_RULE_VERSION_V3: &str = "giveaway-poc-indexed-radix-v3";
pub const GIVEAWAY_RULE_VERSION_V4: &str = "giveaway-poc-indexed-radix-v4-archival";

// PDA Seeds
pub const CONFIG_SEED: &[u8] = b"config";
pub const GIVEAWAY_SEED: &[u8] = b"giveaway";
pub const PARTICIPANT_SEED: &[u8] = b"participant";
pub const VAULT_SEED: &[u8] = b"vault";
pub const WINNERS_LEDGER_SEED: &[u8] = b"winners_root_v2";
pub const FINALIZATION_LEDGER_SEED: &[u8] = b"finalization_root_v2";

/// Disqualification reason codes
pub mod disqualification_reasons {
    /// Did not complete required tasks
    pub const INCOMPLETE_TASKS: u8 = 1;
    /// Provided invalid or fake proof
    pub const INVALID_PROOF: u8 = 2;
    /// Violated giveaway terms
    pub const TERMS_VIOLATION: u8 = 3;
    /// Duplicate or spam entry
    pub const DUPLICATE_ENTRY: u8 = 4;
    /// Other reason (manual review)
    pub const OTHER: u8 = 255;
}
