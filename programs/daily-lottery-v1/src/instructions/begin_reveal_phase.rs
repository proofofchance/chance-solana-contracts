//! Legacy timing mutation is disabled for fixed-rule V1 instances.
use crate::error::Error;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};
pub fn process(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _attestation_secs: u32,
    _upload_secs: u32,
) -> ProgramResult {
    Err(Error::InvalidPhaseTransition.into())
}
