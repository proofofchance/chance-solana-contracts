//! # Config Account State
//!
//! The Config account stores global system configuration including
//! authority, service fees, and default parameters.

use crate::constants::*;
use anchor_lang::prelude::*;

/// Global system configuration
///
/// Stores authority and default parameters for the giveaways program.
/// There is only one Config account per program deployment.
///
/// ## PDA Seeds
/// `["config"]`
#[account]
pub struct Config {
    /// Program authority who can update config and perform admin operations
    pub authority: Pubkey,

    /// Service fee rate in basis points (0-9999, where 10000 = 100%)
    pub service_fee_bps: u16,

    /// Default active duration in seconds for new giveaways
    pub default_active_duration_secs: u32,

    /// Default upload/attestation duration in seconds for new giveaways
    pub default_upload_duration_secs: u32,

    /// When this config was created
    pub created_at_unix: i64,

    /// When this config was last updated
    pub last_updated_unix: i64,

    /// Next sequential giveaway id (starts at 1)
    pub next_giveaway_id: u64,

    /// Reserved space for future fields
    pub registry_program: Pubkey,
    pub registry_config: Pubkey,
}

impl Config {
    /// Size of Config account in bytes
    pub const SIZE: usize = 8 + // discriminator
        32 + // authority
        2 +  // service_fee_bps
        4 +  // default_active_duration_secs
        4 +  // default_upload_duration_secs
        8 +  // created_at_unix
        8 +  // last_updated_unix
        8 +  // next_giveaway_id
        64; // reserved

    /// Initialize a new config
    pub fn initialize(
        &mut self,
        authority: Pubkey,
        service_fee_bps: u16,
        default_active_duration_secs: u32,
        default_upload_duration_secs: u32,
        current_time: i64,
    ) {
        self.authority = authority;
        self.service_fee_bps = service_fee_bps;
        self.default_active_duration_secs = default_active_duration_secs;
        self.default_upload_duration_secs = default_upload_duration_secs;
        self.created_at_unix = current_time;
        self.last_updated_unix = current_time;
        self.next_giveaway_id = 1; // start sequence at 1
        self.registry_program = Pubkey::default();
        self.registry_config = Pubkey::default();
    }

    /// Update service fee rate
    pub fn update_service_fee(&mut self, new_service_fee_bps: u16, current_time: i64) {
        self.service_fee_bps = new_service_fee_bps;
        self.last_updated_unix = current_time;
    }

    /// Update default durations
    pub fn update_defaults(
        &mut self,
        default_active_duration_secs: Option<u32>,
        default_upload_duration_secs: Option<u32>,
        current_time: i64,
    ) {
        if let Some(active_duration) = default_active_duration_secs {
            self.default_active_duration_secs = active_duration;
        }
        if let Some(upload_duration) = default_upload_duration_secs {
            self.default_upload_duration_secs = upload_duration;
        }
        self.last_updated_unix = current_time;
    }

    /// Validate service fee rate
    pub fn validate_service_fee(service_fee_bps: u16) -> bool {
        service_fee_bps <= MAX_SERVICE_FEE_BPS
    }

    /// Validate duration parameters
    pub fn validate_durations(active_duration_secs: u32, upload_duration_secs: u32) -> bool {
        (MIN_ACTIVE_DURATION_SECS..=MAX_ACTIVE_DURATION_SECS).contains(&active_duration_secs)
            && (MIN_UPLOAD_DURATION_SECS..=MAX_UPLOAD_DURATION_SECS).contains(&upload_duration_secs)
    }
}
