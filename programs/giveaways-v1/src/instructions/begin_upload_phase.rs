//! # Begin Upload Phase Instruction
//!
//! Legacy timing mutation is rejected for all fixed-rule builds.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct BeginUploadPhase<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        constraint = authority.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

pub fn process(_ctx: Context<BeginUploadPhase>) -> Result<()> {
    // Fixed-rule releases expose no feature-enabled authority timing override.
    err!(GiveawayError::InvalidInstruction)
}
