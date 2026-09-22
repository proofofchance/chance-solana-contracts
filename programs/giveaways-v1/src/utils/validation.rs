//! # Validation Utilities
//!
//! Common validation functions for account states and instruction parameters.

use crate::error::GiveawayError;
use anchor_lang::prelude::*;

/// Require that an account is writable
pub fn require_writable(account: &AccountInfo) -> Result<()> {
    require!(account.is_writable, GiveawayError::AccountNotWritable);
    Ok(())
}

/// Require that an account is a signer
pub fn require_signer(account: &AccountInfo) -> Result<()> {
    require!(account.is_signer, GiveawayError::MissingSigner);
    Ok(())
}

/// Require that an account is initialized (has data)
pub fn require_initialized(account: &AccountInfo) -> Result<()> {
    require!(
        !account.data_is_empty(),
        GiveawayError::AccountNotInitialized
    );
    Ok(())
}

/// Require that an account is not initialized (empty data)
pub fn require_not_initialized(account: &AccountInfo) -> Result<()> {
    require!(
        account.data_is_empty(),
        GiveawayError::AccountAlreadyInitialized
    );
    Ok(())
}

/// Validate string length
pub fn validate_string_length(text: &str, max_length: usize) -> Result<()> {
    require!(text.len() <= max_length, GiveawayError::TextTooLong);
    Ok(())
}

/// Validate service fee basis points
pub fn validate_service_fee_bps(service_fee_bps: u16) -> Result<()> {
    require!(
        service_fee_bps <= crate::constants::MAX_SERVICE_FEE_BPS,
        GiveawayError::InvalidServiceFee
    );
    Ok(())
}

/// Validate winner count
pub fn validate_winner_count(number_of_winners: u32) -> Result<()> {
    require!(
        (crate::constants::MIN_WINNERS..=crate::constants::MAX_WINNERS)
            .contains(&number_of_winners),
        GiveawayError::InvalidWinnerCount
    );
    Ok(())
}

/// Validate duration parameters
pub fn validate_duration(duration_secs: u32, min_secs: u32, max_secs: u32) -> Result<()> {
    require!(
        duration_secs >= min_secs && duration_secs <= max_secs,
        GiveawayError::InvalidDuration
    );
    Ok(())
}

/// Validate timing sequence
pub fn validate_timing_sequence(start_time: i64, end_time: i64) -> Result<()> {
    require!(start_time < end_time, GiveawayError::InvalidTiming);
    Ok(())
}

/// Validate that a funded giveaway can pay at least the minimum winner amount.
pub fn validate_minimum_winners_pool(
    total_payout_lamports: u64,
    service_fee_bps: u16,
    number_of_winners: u32,
) -> Result<()> {
    let service_fee =
        ((u128::from(total_payout_lamports) * u128::from(service_fee_bps)) / 10_000u128) as u64;
    let winners_pool = total_payout_lamports.saturating_sub(service_fee);
    let minimum_winners_pool = (number_of_winners as u64)
        .checked_mul(crate::constants::MIN_WINNER_PAYOUT_LAMPORTS)
        .ok_or(GiveawayError::MathOverflow)?;
    require!(
        winners_pool >= minimum_winners_pool,
        GiveawayError::InsufficientFunds
    );
    Ok(())
}

/// Validate that current time is within a window
pub fn validate_time_window(current_time: i64, start_time: i64, end_time: i64) -> Result<()> {
    require!(
        current_time >= start_time,
        GiveawayError::ActiveWindowNotStarted
    );
    require!(current_time < end_time, GiveawayError::ActiveWindowClosed);
    Ok(())
}

/// Validate commitment hash (must be non-zero)
pub fn validate_commitment_hash(commitment_hash: &[u8; 32]) -> Result<()> {
    require!(
        commitment_hash != &[0u8; 32],
        GiveawayError::InvalidInstruction
    );
    Ok(())
}

/// Validate disqualification reason code
pub fn validate_reason_code(reason_code: u8) -> Result<()> {
    require!(
        crate::state::Participant::validate_reason_code(reason_code),
        GiveawayError::InvalidReasonCode
    );
    Ok(())
}

/// Check if account has sufficient lamports
pub fn require_sufficient_lamports(account: &AccountInfo, required_lamports: u64) -> Result<()> {
    require!(
        account.lamports() >= required_lamports,
        GiveawayError::InsufficientFunds
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_winners_pool_rejects_zero_lamport_winners() {
        assert!(validate_minimum_winners_pool(1, 0, 2).is_err());
        assert!(validate_minimum_winners_pool(2, 0, 2).is_ok());
    }

    #[test]
    fn minimum_winners_pool_accounts_for_service_fee() {
        assert!(validate_minimum_winners_pool(100, 9_900, 2).is_err());
        assert!(validate_minimum_winners_pool(200, 9_900, 2).is_ok());
    }
}
