//! # Disqualify Participant Instruction
//!
//! Allows giveaway creator to disqualify participants who didn't meet requirements.
//! All disqualifications are logged for transparency.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
    utils::validation::*,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct DisqualifyParticipant<'info> {
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
        constraint = participant_account.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = !participant_account.disqualified @ GiveawayError::AlreadyDisqualified,
    )]
    pub participant_account: Account<'info, Participant>,

    #[account(
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: Signer<'info>,
}

pub fn process(ctx: Context<DisqualifyParticipant>, reason_code: u8) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let participant_account = &mut ctx.accounts.participant_account;
    let creator = &ctx.accounts.creator;
    let clock = Clock::get()?;

    // Validate reason code
    validate_reason_code(reason_code)?;

    // Eligibility is locked once the upload/attestation phase starts. After
    // participants can reveal entropy, creator-side disqualification would be
    // a settlement-gaming surface.
    require!(
        clock.unix_timestamp < giveaway.upload_start_unix,
        GiveawayError::InvalidInstruction
    );

    // Disqualify participant
    participant_account.disqualify(reason_code, clock.unix_timestamp);

    // Update giveaway disqualification count
    giveaway.disqualify_participant()?;

    // Emit event
    crate::events::GiveawayEvent::ParticipantDisqualified {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        participant: participant_account.wallet.to_string(),
        participant_account: participant_account.key().to_string(),
        reason_code,
        disqualified_by: creator.key().to_string(),
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Participant {} disqualified from giveaway {} with reason code {}",
        participant_account.wallet,
        giveaway.id,
        reason_code
    );

    Ok(())
}
