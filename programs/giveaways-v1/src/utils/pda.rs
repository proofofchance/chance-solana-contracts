//! # PDA Derivation Utilities
//!
//! Functions for deriving Program Derived Addresses (PDAs) used by the giveaways program.

use crate::constants::*;
use anchor_lang::prelude::*;

/// Derive config PDA
pub fn derive_config_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED], program_id)
}

/// Derive giveaway PDA
pub fn derive_giveaway_pda(program_id: &Pubkey, config: &Pubkey, giveaway_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GIVEAWAY_SEED, config.as_ref(), &giveaway_id.to_le_bytes()],
        program_id,
    )
}

/// Derive participant PDA
pub fn derive_participant_pda(
    program_id: &Pubkey,
    giveaway: &Pubkey,
    wallet: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[PARTICIPANT_SEED, giveaway.as_ref(), wallet.as_ref()],
        program_id,
    )
}

/// Derive vault PDA
pub fn derive_vault_pda(program_id: &Pubkey, giveaway: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED, giveaway.as_ref()], program_id)
}

/// Derive winners ledger PDA
pub fn derive_winners_ledger_pda(program_id: &Pubkey, giveaway: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[WINNERS_LEDGER_SEED, giveaway.as_ref()], program_id)
}

/// Assert that an account matches the expected PDA
pub fn assert_pda_owned(program_id: &Pubkey, account: &AccountInfo, seeds: &[&[u8]]) -> Result<()> {
    let (expected_pda, _) = Pubkey::find_program_address(seeds, program_id);
    require_keys_eq!(
        account.key(),
        expected_pda,
        crate::error::GiveawayError::InvalidAccount
    );
    require_eq!(
        account.owner,
        program_id,
        crate::error::GiveawayError::InvalidProgram
    );
    Ok(())
}

/// Assert that an account matches a specific expected key
pub fn require_key_match(account: &AccountInfo, expected: &Pubkey) -> Result<()> {
    require_keys_eq!(
        account.key(),
        *expected,
        crate::error::GiveawayError::InvalidAccount
    );
    Ok(())
}
