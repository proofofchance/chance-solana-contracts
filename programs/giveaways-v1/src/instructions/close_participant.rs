//! # Close Participant Instruction
//!
//! Allows a participant to reclaim their participant PDA rent after a giveaway
//! reaches a terminal settled state.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseParticipant<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount,
        constraint = giveaway.settled @ GiveawayError::InvalidInstruction,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        close = participant,
        constraint = participant_account.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = participant_account.wallet == participant.key() @ GiveawayError::InvalidAccount,
    )]
    pub participant_account: Account<'info, Participant>,

    #[account(mut)]
    pub participant: Signer<'info>,
}

pub fn process(ctx: Context<CloseParticipant>) -> Result<()> {
    let giveaway = &ctx.accounts.giveaway;
    let participant_account = &ctx.accounts.participant_account;
    let participant = &ctx.accounts.participant;
    let rent_reclaimed = participant_account.to_account_info().lamports();
    let clock = Clock::get()?;

    crate::events::GiveawayEvent::ParticipantClosed {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        participant: participant.key().to_string(),
        participant_account: participant_account.key().to_string(),
        rent_reclaimed,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Participant {} closed account {} for giveaway {}",
        participant.key(),
        participant_account.key(),
        giveaway.id
    );

    Ok(())
}
