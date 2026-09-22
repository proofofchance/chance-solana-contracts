//! # Attest Uploaded Instruction
//!
//! Allows participants to attest that they have uploaded their proof-of-chance
//! reveal to the service provider. This is part of the anti-censorship mechanism.

use crate::{
    constants::GIVEAWAY_ATTESTATION_DOMAIN_V1,
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
};
use anchor_lang::prelude::*;
use solana_program::{
    ed25519_program,
    sysvar::instructions::{get_instruction_relative, ID as INSTRUCTIONS_SYSVAR_ID},
};

#[derive(Accounts)]
pub struct AttestUploaded<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        constraint = participant_account.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = participant_account.wallet == participant.key() @ GiveawayError::InvalidAccount,
    )]
    pub participant_account: Account<'info, Participant>,

    /// CHECK: The provider receipt and participant PDA bind this wallet. It does
    /// not need to sign, so any fee payer can relay an already-issued receipt.
    #[account()]
    pub participant: UncheckedAccount<'info>,

    /// CHECK: Instructions sysvar used to verify the preceding Ed25519 receipt instruction.
    #[account(address = INSTRUCTIONS_SYSVAR_ID @ GiveawayError::InvalidAccount)]
    pub instructions_sysvar: AccountInfo<'info>,
}

pub fn process(ctx: Context<AttestUploaded>) -> Result<()> {
    let _config = &ctx.accounts.config;
    let giveaway = &mut ctx.accounts.giveaway;
    let participant_account = &mut ctx.accounts.participant_account;
    let participant = &ctx.accounts.participant;
    let instructions_sysvar = &ctx.accounts.instructions_sysvar;
    let clock = Clock::get()?;

    // Validate giveaway is not settled
    require!(!giveaway.settled, GiveawayError::GiveawayAlreadySettled);

    // Validate we're in upload/attestation phase
    require!(
        giveaway.is_in_upload_phase(clock.unix_timestamp),
        GiveawayError::UploadWindowClosed
    );

    // Validate participant hasn't already attested
    require!(
        !participant_account.attested_uploaded,
        GiveawayError::AlreadyAttested
    );

    // Validate participant is not disqualified
    require!(
        !participant_account.disqualified,
        GiveawayError::AlreadyDisqualified
    );

    verify_provider_receipt(
        instructions_sysvar,
        &giveaway.provider_authority,
        &giveaway.key(),
        &participant.key(),
        &participant_account.commitment_hash,
    )?;

    // Mark as attested
    participant_account.mark_attested(clock.unix_timestamp);

    // Update giveaway attestation count
    giveaway.add_attestation()?;

    // Emit event
    crate::events::GiveawayEvent::AttestationSubmitted {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        participant: participant.key().to_string(),
        participant_account: participant_account.key().to_string(),
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Participant {} attested upload for giveaway {}",
        participant.key(),
        giveaway.id
    );

    Ok(())
}

fn verify_provider_receipt(
    instructions_sysvar: &AccountInfo,
    authority: &Pubkey,
    giveaway: &Pubkey,
    participant: &Pubkey,
    commitment_hash: &[u8; 32],
) -> Result<()> {
    let mut expected_message = GIVEAWAY_ATTESTATION_DOMAIN_V1.to_vec();
    expected_message.extend_from_slice(giveaway.as_ref());
    expected_message.extend_from_slice(participant.as_ref());
    expected_message.extend_from_slice(commitment_hash);

    for relative_index in -10..=-1 {
        if let Ok(ix) = get_instruction_relative(relative_index, instructions_sysvar) {
            if ix.program_id != ed25519_program::id() {
                continue;
            }
            if let Some((pubkey, message)) = parse_ed25519_ix(&ix.data) {
                if pubkey.as_ref() == authority.as_ref() && message == expected_message.as_slice() {
                    return Ok(());
                }
            }
        }
    }

    Err(GiveawayError::InvalidAttestation.into())
}

fn parse_ed25519_ix(data: &[u8]) -> Option<(&[u8; 32], &[u8])> {
    if data.len() < 16 || data[0] != 1 {
        return None;
    }

    let read_u16 = |offset: usize| -> Option<u16> {
        Some(u16::from_le_bytes([
            *data.get(offset)?,
            *data.get(offset + 1)?,
        ]))
    };

    let signature_offset = read_u16(2)? as usize;
    let signature_instruction_index = read_u16(4)?;
    let pubkey_offset = read_u16(6)? as usize;
    let pubkey_instruction_index = read_u16(8)?;
    let message_offset = read_u16(10)? as usize;
    let message_size = read_u16(12)? as usize;
    let message_instruction_index = read_u16(14)?;

    if signature_instruction_index != u16::MAX
        || pubkey_instruction_index != u16::MAX
        || message_instruction_index != u16::MAX
    {
        return None;
    }

    if signature_offset + 64 > data.len()
        || pubkey_offset + 32 > data.len()
        || message_offset + message_size > data.len()
    {
        return None;
    }

    let pubkey = data
        .get(pubkey_offset..pubkey_offset + 32)?
        .try_into()
        .ok()?;
    let message = data.get(message_offset..message_offset + message_size)?;
    Some((pubkey, message))
}
