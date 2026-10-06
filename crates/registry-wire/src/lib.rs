//! Versioned registry wire types. Addresses are bytes so both Solana SDK generations
//! share the exact same encoding without linking each other's runtime dependencies.
use borsh::{BorshDeserialize, BorshSerialize};
pub use borsh::{from_slice, to_vec};
pub const SCHEMA: u16 = 1;
pub const DAILY: u8 = 1;
pub const GIVEAWAY: u8 = 2;
pub const REGISTERED: u8 = 1;
pub const ACTIVE: u8 = 2;
pub const PAUSED: u8 = 3;
pub const DISCONTINUED: u8 = 4;
pub const CONFIG_TAG: [u8; 8] = *b"CHREG001";
pub const RELEASE_TAG: [u8; 8] = *b"CHREL001";
pub const RECORD_TAG: [u8; 8] = *b"CHREC001";
pub const INSTANCE_TAG: [u8; 8] = *b"CHINS001";
pub const KEY_TAG: [u8; 8] = *b"CHKEY001";
pub const MAX_REASON: usize = 384;

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub enum Instruction {
    Initialize { guardian: [u8; 32], activation_delay: u32 },
    Register { domain: u8, series: [u8; 32], source_hash: [u8; 32], executable_hash: [u8; 32] },
    Activate { reason: String },
    Pause { reason: String },
    Discontinue { reason: String },
    SetSuccessor { reason: String },
    RecordInstance { day_or_nonce: u64, rules_hash: [u8; 32] },
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Config {
    pub tag: [u8; 8],
    pub schema: u16,
    pub authority: [u8; 32],
    pub guardian: [u8; 32],
    pub activation_delay: u32,
    pub release_count: u64,
    pub record_count: u64,
    pub instance_count: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Release {
    pub tag: [u8; 8],
    pub registry: [u8; 32],
    pub program: [u8; 32],
    pub domain: u8,
    pub series: [u8; 32],
    pub source_hash: [u8; 32],
    /// SHA-256 of deployed ELF after trailing-zero trimming (solana-verify convention).
    /// Activation checks this against immutable ProgramData; source match is separate.
    pub executable_hash: [u8; 32],
    pub sequence: u64,
    pub registered_slot: u64,
    pub eligible_at: i64,
    pub status: u8,
    pub successor: [u8; 32],
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Record {
    pub tag: [u8; 8],
    pub registry: [u8; 32],
    pub sequence: u64,
    pub release: [u8; 32],
    pub actor: [u8; 32],
    pub slot: u64,
    pub timestamp: i64,
    pub status: u8,
    pub successor: [u8; 32],
    pub reason: String,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Instance {
    pub tag: [u8; 8],
    pub registry: [u8; 32],
    pub sequence: u64,
    pub release: [u8; 32],
    pub owner_program: [u8; 32],
    pub instance: [u8; 32],
    pub creator: [u8; 32],
    pub business_key: [u8; 32],
    pub rules_hash: [u8; 32],
    pub created_slot: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct BusinessKey {
    pub tag: [u8; 8],
    pub entry: [u8; 32],
}
