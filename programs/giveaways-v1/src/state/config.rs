//! # Config Account State
//!
//! The Config account stores global system configuration including
//! authority, service fees, and default parameters.

use crate::constants::*;
use anchor_lang::prelude::*;
use sha2::{Digest, Sha256};

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
    /// Digest of the effective creation context and creator-supplied terms.
    #[allow(clippy::too_many_arguments)]
    pub fn creation_hash(
        &self,
        program: &Pubkey,
        config: &Pubkey,
        creator: &Pubkey,
        giveaway_id: u64,
        payout_lamports: u64,
        winners: u32,
        active_start: i64,
        active_deadline: i64,
        upload_duration: u32,
        creator_nonce: u64,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"CHANCE_GIVEAWAY_CREATION_V1");
        hash.update(program.as_ref());
        hash.update(config.as_ref());
        hash.update(self.registry_program.as_ref());
        hash.update(self.registry_config.as_ref());
        hash.update(self.authority.as_ref());
        hash.update(self.service_fee_bps.to_le_bytes());
        hash.update(creator.as_ref());
        hash.update(giveaway_id.to_le_bytes());
        hash.update(payout_lamports.to_le_bytes());
        hash.update(winners.to_le_bytes());
        hash.update(active_start.to_le_bytes());
        hash.update(active_deadline.to_le_bytes());
        hash.update(upload_duration.to_le_bytes());
        hash.update(creator_nonce.to_le_bytes());
        hash.finalize().into()
    }

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

#[cfg(test)]
mod creation_hash_tests {
    use super::*;

    #[test]
    fn creator_terms_and_mutable_provider_settings_change_digest() {
        let program = Pubkey::new_unique();
        let config_key = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let mut config = Config {
            authority: Pubkey::new_unique(),
            service_fee_bps: 500,
            default_active_duration_secs: 86_400,
            default_upload_duration_secs: 86_400,
            created_at_unix: 1,
            last_updated_unix: 1,
            next_giveaway_id: 1,
            registry_program: Pubkey::new_unique(),
            registry_config: Pubkey::new_unique(),
        };
        let digest = |settings: &Config, nonce| {
            settings.creation_hash(
                &program,
                &config_key,
                &creator,
                1,
                1_000_000,
                2,
                100,
                200,
                3_600,
                nonce,
            )
        };
        let expected = digest(&config, 7);
        assert_eq!(expected, digest(&config, 7));
        assert_ne!(expected, digest(&config, 8));
        assert_ne!(
            expected,
            config.creation_hash(
                &program,
                &config_key,
                &Pubkey::new_unique(),
                1,
                1_000_000,
                2,
                100,
                200,
                3_600,
                7,
            )
        );
        config.service_fee_bps = 600;
        assert_ne!(expected, digest(&config, 7));
        config.service_fee_bps = 500;
        config.authority = Pubkey::new_unique();
        assert_ne!(expected, digest(&config, 7));
    }
}
