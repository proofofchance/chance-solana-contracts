//! # Config Account State
//!
//! The Config account stores global lottery system configuration and is the root authority
//! for all lottery operations. It uses PDA seeds `["config"]`.
//!
//! ## Key Features
//! - Authority management for lottery operations
//! - Ticket pricing configuration (immutable after init)
//! - Service charge settings (updatable by authority)
//! - Lottery counter for unique ID generation across concurrent lotteries

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

/// Global configuration for the daily lottery system
///
/// This account is created once during program initialization and stores
/// system-wide settings that govern all lottery operations.
///
/// ## PDA Seeds
/// `["config"]`
///
/// ## Authority Model
/// Only the `authority` pubkey can:
/// - Create new lotteries
/// - Update service charge rates
/// - Adjust reveal windows (emergency use)
/// - Settle lotteries
#[derive(BorshSerialize, BorshDeserialize, Debug, Default, Clone)]
pub struct Config {
    /// The authority pubkey that can perform administrative operations
    /// Set during initialization and cannot be changed
    pub authority: Pubkey,

    /// Price per lottery ticket in lamports
    /// Set during initialization and cannot be changed to ensure fairness
    pub ticket_price_lamports: u64,

    /// Service charge in basis points (0-9999, where 10000 = 100%)
    /// Can be updated by authority to adjust platform fees
    pub service_charge_bps: u16,

    /// Total number of lotteries created (used for unique ID generation)
    /// Incremented each time a new lottery is created
    pub lottery_count: u64,

    /// Default buy window length in seconds (e.g., 24h)
    pub buy_window_secs: u32,

    /// Default upload window length in seconds (e.g., 24h)
    pub upload_window_secs: u32,

    /// Upper bound for winners count to size on-chain bitmap allocation
    /// Used to pre-allocate sufficient space in the Lottery account at creation time
    pub max_winners_cap: u32,

    /// One-time registry binding for this new program release.
    pub registry_program: Pubkey,
    pub registry_config: Pubkey,
}

impl Config {
    /// Effective creation rules, excluding the mutable allocation counter.
    /// All integers use little endian; the program/config/registry bind this digest
    /// to one release. No account layout or legacy instruction encoding changes.
    pub fn preset_hash(&self, program: &Pubkey, config: &Pubkey) -> [u8; 32] {
        solana_sha256_hasher::hashv(&[
            b"CHANCE_DAILY_PRESET_V1",
            program.as_ref(),
            config.as_ref(),
            self.registry_program.as_ref(),
            self.registry_config.as_ref(),
            self.authority.as_ref(),
            &self.ticket_price_lamports.to_le_bytes(),
            &self.service_charge_bps.to_le_bytes(),
            &self.buy_window_secs.to_le_bytes(),
            &self.upload_window_secs.to_le_bytes(),
            &self.max_winners_cap.to_le_bytes(),
        ])
        .to_bytes()
    }

    /// Validates that the service charge is within acceptable bounds
    pub fn validate_service_charge(bps: u16) -> bool {
        bps < 10_000 // Must be less than 100%
    }

    /// Validates the configured winner-count capacity for per-lottery ledgers.
    pub fn validate_max_winners_cap(max_winners_cap: u32) -> bool {
        max_winners_cap > 0 && max_winners_cap <= crate::state::sizes::MAX_WINNERS as u32
    }

    /// Returns the maximum winner count this config can honor for a lottery.
    pub fn effective_max_winners(&self, participants_count: u64) -> u64 {
        if participants_count <= 1 {
            return 1;
        }

        let configured_cap = (self.max_winners_cap as u64)
            .min(crate::state::sizes::MAX_WINNERS as u64)
            .max(1);
        configured_cap.min(participants_count.saturating_sub(1))
    }

    /// Increments the lottery count and returns the new lottery ID
    pub fn next_lottery_id(&mut self) -> Result<u64, crate::error::Error> {
        self.lottery_count = self
            .lottery_count
            .checked_add(1)
            .ok_or(crate::error::Error::MathOverflow)?;
        Ok(self.lottery_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_survives_allocation_but_detects_rule_and_release_changes() {
        let program = Pubkey::new_unique();
        let address = Pubkey::new_unique();
        let mut config = Config {
            ticket_price_lamports: 10,
            ..Config::default()
        };
        let expected = config.preset_hash(&program, &address);
        config.next_lottery_id().unwrap();
        assert_eq!(expected, config.preset_hash(&program, &address));
        config.service_charge_bps = 500;
        assert_ne!(expected, config.preset_hash(&program, &address));
        assert_ne!(
            config.preset_hash(&program, &address),
            config.preset_hash(&Pubkey::new_unique(), &address)
        );
        assert_ne!(
            config.preset_hash(&program, &address),
            config.preset_hash(&program, &Pubkey::new_unique())
        );
    }

    #[test]
    fn test_validate_service_charge() {
        assert!(Config::validate_service_charge(0));
        assert!(Config::validate_service_charge(500)); // 5%
        assert!(Config::validate_service_charge(9999)); // 99.99%
        assert!(!Config::validate_service_charge(10000)); // 100%
        assert!(!Config::validate_service_charge(15000)); // 150%
    }

    #[test]
    fn test_validate_max_winners_cap() {
        assert!(Config::validate_max_winners_cap(1));
        assert!(Config::validate_max_winners_cap(
            crate::state::sizes::MAX_WINNERS as u32
        ));
        assert!(!Config::validate_max_winners_cap(0));
        assert!(!Config::validate_max_winners_cap(
            crate::state::sizes::MAX_WINNERS as u32 + 1
        ));
    }

    #[test]
    fn test_effective_max_winners() {
        let mut config = Config {
            max_winners_cap: 32,
            ..Config::default()
        };
        assert_eq!(config.effective_max_winners(1), 1);
        assert_eq!(config.effective_max_winners(2), 1);
        assert_eq!(config.effective_max_winners(40), 32);

        config.max_winners_cap = 0;
        assert_eq!(config.effective_max_winners(10), 1);
    }

    // lifecycle constraints removed; multiple concurrent lotteries supported
}
