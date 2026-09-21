//! # Extend Active Deadline Instruction
//!
//! Allows creator to extend the participation deadline before it passes.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ExtendActiveDeadline<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: Signer<'info>,
}

pub fn process(_ctx: Context<ExtendActiveDeadline>, _new_deadline_unix: i64) -> Result<()> {
    // A new instance is required for changed terms; active deadlines are immutable.
    err!(GiveawayError::InvalidInstruction)
}
