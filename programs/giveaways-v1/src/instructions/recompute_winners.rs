//! # Recompute Winners Instruction
//!
//! Legacy recomputation is disabled for the scalable finalization protocol.
//! Eligibility changes must happen before upload starts; finalization then
//! processes the resulting reveal-included participant set in chunks.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway, WinnersLedger},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct RecomputeWinners<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = !giveaway.winners_locked @ GiveawayError::WinnersLocked,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        constraint = signer.key() == giveaway.creator || signer.key() == giveaway.provider_authority @ GiveawayError::Unauthorized
    )]
    pub signer: Signer<'info>,
}

pub fn process(ctx: Context<RecomputeWinners>) -> Result<()> {
    let _ = ctx;
    msg!("Legacy recompute_winners is disabled; use chunked finalize_winners before locking");
    err!(GiveawayError::InvalidInstruction)
}
