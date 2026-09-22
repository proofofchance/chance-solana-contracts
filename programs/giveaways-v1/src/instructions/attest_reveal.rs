//! Permissionless participant reveal escape hatch.
//!
//! A participant can verify and include their committed reveal directly on
//! chain during the upload window without obtaining a provider receipt.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
    utils::crypto::{
        build_reveal_plaintext, compute_reveal_digest, verify_reveal, xor_reveal_digests,
    },
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct AttestReveal<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        seeds = [PARTICIPANT_SEED, giveaway.key().as_ref(), participant.key().as_ref()],
        bump,
        constraint = participant_account.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = participant_account.wallet == participant.key() @ GiveawayError::InvalidAccount,
    )]
    pub participant_account: Account<'info, Participant>,

    pub participant: Signer<'info>,
}

pub fn process(ctx: Context<AttestReveal>, lucky_words: String, salt: Vec<u8>) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let participant_account = &mut ctx.accounts.participant_account;
    let participant = &ctx.accounts.participant;
    let clock = Clock::get()?;

    require!(
        giveaway.is_in_upload_phase(clock.unix_timestamp),
        GiveawayError::UploadWindowClosed
    );
    require!(
        !participant_account.disqualified,
        GiveawayError::AlreadyDisqualified
    );
    require!(
        !participant_account.reveal_included,
        GiveawayError::RevealAlreadyIncluded
    );
    require!(
        lucky_words.len() <= MAX_LUCKY_WORDS_LEN && salt.len() <= MAX_SALT_LEN,
        GiveawayError::TextTooLong
    );
    require!(
        verify_reveal(&participant_account.commitment_hash, &lucky_words, &salt,)?,
        GiveawayError::InvalidReveal
    );

    let plaintext = build_reveal_plaintext(&lucky_words, &salt)?;
    let reveal_digest = compute_reveal_digest(&participant.key(), &plaintext);
    let aggregate_hash = xor_reveal_digests(giveaway.poc_aggregate_hash, &[reveal_digest]);

    if !participant_account.attested_uploaded {
        participant_account.mark_attested(clock.unix_timestamp);
        giveaway.add_attestation()?;
        crate::events::GiveawayEvent::AttestationSubmitted {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            participant: participant.key().to_string(),
            participant_account: participant_account.key().to_string(),
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    participant_account.include_verified_reveal(reveal_digest, clock.unix_timestamp);
    giveaway.add_uploaded_reveals(1, aggregate_hash)?;

    crate::events::GiveawayEvent::RevealsUploaded {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        authority: participant.key().to_string(),
        batch_size: 1,
        total_reveals_uploaded: giveaway.provider_uploaded_count,
        total_attested: giveaway.attested_count,
        aggregate_hash: hex::encode(aggregate_hash),
        uploads_complete: giveaway.uploads_complete,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    Ok(())
}
