//! # Upload Reveals Instruction
//!
//! Allows the service provider to upload batches of participant reveals
//! for proof-of-chance entropy generation. After the upload deadline, if
//! accepted/attested reveals were omitted, the same instruction becomes
//! permissionless remediation: any signer can include missing attested reveals.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
    utils::{
        crypto::{
            build_reveal_plaintext, compute_reveal_digest, verify_reveal, xor_reveal_digests,
        },
        pda::assert_pda_owned,
    },
};
use anchor_lang::prelude::*;
use std::io::Cursor;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RevealData {
    pub participant: Pubkey,
    pub lucky_words: String,
    pub salt: Vec<u8>,
}

#[derive(Accounts)]
pub struct UploadReveals<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount,
    )]
    pub giveaway: Account<'info, Giveaway>,

    pub authority: Signer<'info>,
}

pub fn process(ctx: Context<UploadReveals>, reveals: Vec<RevealData>) -> Result<()> {
    let _config = &ctx.accounts.config;
    let giveaway = &mut ctx.accounts.giveaway;
    let uploader = &ctx.accounts.authority;
    let clock = Clock::get()?;

    // Validate batch size
    require!(
        !reveals.is_empty() && reveals.len() <= MAX_REVEALS_PER_BATCH,
        GiveawayError::InvalidInstruction
    );

    // Keep provider-held reveals private until the upload deadline, unless all
    // eligible participants have already attested. After the deadline, omitted
    // attested reveals can be included by any signer during remediation.
    let upload_elapsed = clock.unix_timestamp >= giveaway.upload_deadline_unix;
    let is_authority = uploader.key() == giveaway.provider_authority;
    let can_remediate = !is_authority
        && upload_elapsed
        && giveaway.has_missing_attested_reveals()
        && !giveaway.remediation_expired(clock.unix_timestamp);

    require!(
        !giveaway.remediation_expired(clock.unix_timestamp),
        GiveawayError::InvalidInstruction
    );
    require!(is_authority || can_remediate, GiveawayError::Unauthorized);

    if can_remediate && giveaway.remediation_start_unix == 0 {
        giveaway.begin_remediation(clock.unix_timestamp);
        crate::events::GiveawayEvent::RevealRemediationBegan {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            included_reveals_count: giveaway.provider_uploaded_count,
            attested_count: giveaway.attested_count,
            remediation_start_unix: giveaway.remediation_start_unix,
            remediation_deadline_unix: giveaway.remediation_deadline_unix,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    require!(
        giveaway.reveal_publication_allowed(clock.unix_timestamp),
        GiveawayError::RevealPublicationNotReady
    );
    require!(!giveaway.winners_computed, GiveawayError::WinnersLocked);

    let mut valid_reveals = 0u64;
    let mut reveal_digests = Vec::with_capacity(reveals.len());
    let mut seen = std::collections::HashSet::new();

    // Process each reveal in the batch
    for reveal in reveals.iter() {
        require!(
            seen.insert(reveal.participant),
            GiveawayError::InvalidReveal
        );

        // Validate reveal data lengths
        require!(
            reveal.lucky_words.len() <= MAX_LUCKY_WORDS_LEN && reveal.salt.len() <= MAX_SALT_LEN,
            GiveawayError::TextTooLong
        );

        // Get participant account from remaining accounts
        let participant_account_info = ctx
            .remaining_accounts
            .iter()
            .find(|acc| acc.key() == reveal.participant);

        let participant_info = participant_account_info.ok_or(GiveawayError::InvalidAccount)?;
        require!(
            participant_info.is_writable,
            GiveawayError::AccountNotWritable
        );
        require_eq!(
            participant_info.owner,
            ctx.program_id,
            GiveawayError::InvalidProgram
        );

        // Deserialize participant account
        let mut participant = {
            let account_data = participant_info.data.borrow();
            let mut participant_data = &account_data[..];
            Participant::try_deserialize(&mut participant_data)
                .map_err(|_| GiveawayError::AccountNotInitialized)?
        };

        assert_pda_owned(
            ctx.program_id,
            participant_info,
            &[
                PARTICIPANT_SEED,
                giveaway.key().as_ref(),
                participant.wallet.as_ref(),
            ],
        )?;
        require_keys_eq!(
            participant.giveaway,
            giveaway.key(),
            GiveawayError::InvalidAccount
        );
        require!(
            !participant.reveal_included,
            GiveawayError::RevealAlreadyIncluded
        );
        require!(
            !participant.disqualified,
            GiveawayError::AlreadyDisqualified
        );
        require!(participant.attested_uploaded, GiveawayError::NotAttested);

        // Verify reveal against commitment
        require!(
            verify_reveal(
                &participant.commitment_hash,
                &reveal.lucky_words,
                &reveal.salt,
            )?,
            GiveawayError::InvalidReveal
        );

        let plaintext = build_reveal_plaintext(&reveal.lucky_words, &reveal.salt)?;
        let reveal_digest = compute_reveal_digest(&participant.wallet, &plaintext);
        reveal_digests.push(reveal_digest);

        // Mark reveal as included and write back.
        participant.include_verified_reveal(reveal_digest, clock.unix_timestamp);

        let mut account_data = participant_info.data.borrow_mut();
        account_data.fill(0);
        let mut writer = Cursor::new(&mut account_data[..]);
        participant.try_serialize(&mut writer)?;

        valid_reveals = valid_reveals
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
    }

    // Update giveaway reveal count
    let aggregate_hash = xor_reveal_digests(giveaway.poc_aggregate_hash, &reveal_digests);
    giveaway.add_uploaded_reveals(valid_reveals, aggregate_hash)?;
    if giveaway.uploads_complete && giveaway.remediation_start_unix > 0 {
        crate::events::GiveawayEvent::RevealRemediationCompleted {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            included_reveals_count: giveaway.provider_uploaded_count,
            attested_count: giveaway.attested_count,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    // Emit event
    crate::events::GiveawayEvent::RevealsUploaded {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        authority: uploader.key().to_string(),
        batch_size: u32::try_from(valid_reveals).map_err(|_| GiveawayError::MathOverflow)?,
        total_reveals_uploaded: giveaway.provider_uploaded_count,
        total_attested: giveaway.attested_count,
        aggregate_hash: hex::encode(aggregate_hash),
        uploads_complete: giveaway.uploads_complete,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Uploaded {} valid reveals for giveaway {} (total: {})",
        valid_reveals,
        giveaway.id,
        giveaway.provider_uploaded_count
    );

    Ok(())
}
