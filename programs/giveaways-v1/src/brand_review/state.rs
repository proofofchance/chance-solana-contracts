use anchor_lang::prelude::*;

pub const MAX_TASKS: usize = 32;
pub const MAX_EVIDENCE: usize = 256;
pub const MAX_VERSIONS: u8 = 4;
pub const MAX_REGISTERED: u64 = 4_096;
pub const MAX_WINNERS: u32 = 1_000;
pub const WAVE_SIZE: usize = 32;
pub const OPEN: u8 = 0;
pub const POOL_SCAN: u8 = 1;
pub const REVIEW: u8 = 2;
pub const VESTED: u8 = 3;
pub const REFUNDED: u8 = 4;
pub const BUILDING: u8 = 0;
pub const READY: u8 = 1;

#[account]
pub struct Config {
    pub authority: Pubkey,
    pub fee_bps: u16,
    pub network_domain: [u8; 32],
    pub next_giveaway_id: u64,
    pub registry_program: Pubkey,
    pub registry_config: Pubkey,
}
impl Config {
    pub const SPACE: usize = 8 + 32 + 2 + 32 + 8 + 64;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskInput {
    pub task_id: u16,
    pub title: String,
    pub instructions: String,
    pub target_uri: String,
    pub evidence_prompt: String,
    pub max_evidence_bytes: u16,
}
#[account]
pub struct Rulebook {
    pub creator: Pubkey,
    pub nonce: u64,
    pub task_count: u16,
    pub appended: u16,
    pub total_bytes: u32,
    pub root: [u8; 32],
    pub sealed: bool,
    pub instance: Pubkey,
}
impl Rulebook {
    pub const SPACE: usize = 8 + 32 + 8 + 2 + 2 + 4 + 32 + 1 + 32;
}
#[account]
pub struct Task {
    pub rulebook: Pubkey,
    pub input: TaskInput,
    pub hash: [u8; 32],
}
impl Task {
    pub const SPACE: usize = 8 + 32 + 2 + 4 * 4 + 64 + 256 + 128 + 128 + 2 + 32;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Terms {
    pub funding: u64,
    pub winners: u32,
    pub entry_start: i64,
    pub entry_end: i64,
    pub upload_end: i64,
    pub remediation_end: i64,
    pub opening_cutoff: i64,
    pub absolute_deadline: i64,
    pub review_seconds: u32,
    pub finalization_reserve_seconds: u32,
}
#[account]
pub struct Giveaway {
    pub id: u64,
    pub config: Pubkey,
    pub rulebook: Pubkey,
    pub creator: Pubkey,
    pub brand: Pubkey,
    pub host: Pubkey,
    pub vault: Pubkey,
    pub rules_hash: [u8; 32],
    pub network_domain: [u8; 32],
    pub task_root: [u8; 32],
    pub task_count: u16,
    pub fee_bps: u16,
    pub terms: Terms,
    pub status: u8,
    pub registered_count: u64,
    pub sealed_count: u64,
    pub attested_count: u64,
    pub included_count: u64,
    pub aggregate: [u8; 32],
    pub base_pool_count: u64,
    pub pool_hash: [u8; 32],
    pub seed: [u8; 32],
    pub consumed_count: u64,
    pub approved_count: u32,
    pub accepted_root: [u8; 32],
    pub pending: bool,
    pub pending_deadline: i64,
    pub per_winner: u64,
    pub fee: u64,
    pub remainder: u64,
    pub outcome_hash: [u8; 32],
    pub paid_winners: u32,
    pub fee_paid: bool,
    pub remainder_paid: bool,
}
impl Giveaway {
    pub const SPACE: usize = 1_024;
}
#[account]
pub struct Vault {
    pub giveaway: Pubkey,
}
impl Vault {
    pub const SPACE: usize = 40;
}

#[account]
pub struct Participant {
    pub giveaway: Pubkey,
    pub wallet: Pubkey,
    pub index: u64,
    pub commitment: [u8; 32],
    pub revision: u32,
    pub versions: [u8; MAX_TASKS],
    pub lengths: [u16; MAX_TASKS],
    pub current_bytes: u32,
    pub sealed: bool,
    pub sealed_revision: u32,
    pub evidence_root: [u8; 32],
    pub attested: bool,
    pub revealed: bool,
    pub contribution: [u8; 32],
    pub bump: u8,
}
impl Participant {
    pub const SPACE: usize = 8 + 32 * 5 + 8 + 4 + 32 + 64 + 4 + 1 + 4 + 1 + 1 + 1;
}
#[account]
pub struct Evidence {
    pub participant: Pubkey,
    pub task_id: u16,
    pub version: u8,
    pub text: String,
    pub hash: [u8; 32],
    pub published_at: i64,
    pub bump: u8,
}
impl Evidence {
    pub const SPACE: usize = 8 + 32 + 2 + 1 + 4 + MAX_EVIDENCE + 32 + 8 + 1;
}
#[account]
pub struct SealSession {
    pub participant: Pubkey,
    pub revision: u32,
    pub cursor: u16,
    pub root: [u8; 32],
    pub total_bytes: u32,
    pub active: bool,
}
impl SealSession {
    pub const SPACE: usize = 8 + 32 + 4 + 2 + 32 + 4 + 1;
}
#[account]
pub struct Ranking {
    pub giveaway: Pubkey,
    pub phase: u8,
    pub cursor: u64,
    pub wave: u32,
    pub frontier: [u8; 64],
    pub has_frontier: bool,
    pub remaining: u64,
    pub keys: Vec<[u8; 64]>,
    pub next_key: u16,
}
impl Ranking {
    pub const SPACE: usize = 8 + 32 + 1 + 8 + 4 + 64 + 1 + 8 + 4 + WAVE_SIZE * 64 + 2;
}
#[account]
pub struct Candidate {
    pub giveaway: Pubkey,
    pub ordinal: u64,
    pub key: [u8; 64],
    pub wallet: Pubkey,
    pub entry_revision: u32,
    pub evidence_root: [u8; 32],
    pub slot: u32,
    pub opened_at: i64,
    pub deadline: i64,
    pub verdict: u8,
    pub reason: u8,
    pub note: String,
    pub task_mask: u32,
    pub decided_at: i64,
}
impl Candidate {
    pub const SPACE: usize = 8 + 32 + 8 + 64 + 32 + 4 + 32 + 4 + 8 + 8 + 1 + 1 + 4 + 128 + 4 + 8;
}
#[account]
pub struct WinnerSlot {
    pub giveaway: Pubkey,
    pub slot: u32,
    pub wallet: Pubkey,
    pub ordinal: u64,
    pub revision: u32,
    pub evidence_root: [u8; 32],
    pub approved: bool,
    pub paid: bool,
}
impl WinnerSlot {
    pub const SPACE: usize = 8 + 32 + 4 + 32 + 8 + 4 + 32 + 1 + 1;
}

#[error_code]
pub enum GiveawayError {
    #[msg("Invalid account or canonical binding")]
    Account,
    #[msg("Unauthorized role")]
    Unauthorized,
    #[msg("Invalid phase or immutable state")]
    State,
    #[msg("Deadline or entry window closed")]
    Time,
    #[msg("Invalid text, index, count or resource limit")]
    Limit,
    #[msg("Checked creation terms changed")]
    CreationMismatch,
    #[msg("Incomplete, stale or out-of-order coverage")]
    Coverage,
    #[msg("Invalid phrase reveal or receipt")]
    Reveal,
    #[msg("Accounting overflow or insufficient recognized funding")]
    Funds,
}

#[event]
pub struct Audit {
    pub kind: u8,
    pub instance: Pubkey,
    pub actor: Pubkey,
    pub sequence: u64,
    pub hash: [u8; 32],
    pub value: u64,
    pub timestamp: i64,
}

/// Complete bounded public records let finalized indexers rebuild without historical account RPC.
#[event]
pub struct BrandReviewRecord {
    pub kind: u8,
    pub address: Pubkey,
    pub data: Vec<u8>,
}
