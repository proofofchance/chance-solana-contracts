//! # Participate Instruction
//!
//! Allows participants to submit their entry with proof text and commitment hash.
//! Can be called multiple times to update entry before deadline.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
    utils::validation::*,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct Participate<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        init_if_needed,
        payer = participant,
        space = Participant::MAX_SIZE,
        seeds = [PARTICIPANT_SEED, giveaway.key().as_ref(), participant.key().as_ref()],
        bump
    )]
    pub participant_account: Account<'info, Participant>,

    #[account(mut)]
    pub participant: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn process(
    ctx: Context<Participate>,
    commitment_hash: [u8; 32],
    proof_text: String,
) -> Result<()> {
    let accounts = ctx.accounts;
    let _config = &accounts.config;
    let giveaway = &mut accounts.giveaway;
    let participant_account = &mut accounts.participant_account;
    let participant = &accounts.participant;
    let clock = Clock::get()?;

    // Validate giveaway is not settled
    require!(!giveaway.settled, GiveawayError::GiveawayAlreadySettled);

    // Validate we're in active phase
    require!(
        giveaway.is_in_active_phase(clock.unix_timestamp),
        GiveawayError::ActiveWindowClosed
    );

    // Validate parameters
    validate_string_length(&proof_text, MAX_PROOF_TEXT_LEN)?;
    validate_commitment_hash(&commitment_hash)?;

    let is_update = participant_account.wallet != Pubkey::default();

    if is_update {
        // Update existing participation
        participant_account.update(commitment_hash, proof_text.clone(), clock.unix_timestamp);
    } else {
        require!(
            giveaway.participants_count < MAX_PARTICIPANTS,
            GiveawayError::TooManyParticipants
        );
        // New participation
        participant_account.initialize(
            giveaway.key(),
            participant.key(),
            commitment_hash,
            proof_text.clone(),
            clock.unix_timestamp,
            giveaway.participants_count,
        );

        // Increment participant count
        giveaway.add_participant()?;
    }

    // Emit event
    crate::events::GiveawayEvent::ParticipationSubmitted {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        participant: participant.key().to_string(),
        participant_account: participant_account.key().to_string(),
        commitment_hash: hex::encode(commitment_hash),
        proof_text,
        is_update,
        participant_index: participant_account.participant_index(),
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Participant {} {} in giveaway {}",
        participant.key(),
        if is_update { "updated entry" } else { "joined" },
        giveaway.id
    );

    Ok(())
}
