//! Permissionless direct threshold-verified giveaway payouts.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant, WinnersLedger},
    utils::{account::transfer_lamports, crypto::compute_candidate_key, pda::assert_pda_owned},
};
use anchor_lang::prelude::*;
use std::io::Cursor;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WinnerProof {
    pub winner: Pubkey,
    pub amount: u64,
    pub winner_index: u32,
    /// Legacy ABI field; protocol v2 verifies the participant PDA directly.
    pub merkle_proof: Vec<Vec<u8>>,
}

#[derive(Accounts)]
pub struct SettlePayoutBatch<'info> {
    #[account()]
    pub config: Account<'info, Config>,
    #[account(mut, constraint = giveaway.winners_computed @ GiveawayError::WinnersNotComputed,
        constraint = giveaway.winners_locked @ GiveawayError::WinnersNotLocked)]
    pub giveaway: Account<'info, Giveaway>,
    /// CHECK: Validated against giveaway state.
    #[account(mut, constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount)]
    pub vault: AccountInfo<'info>,
    #[account(mut, constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = winners_ledger.locked @ GiveawayError::WinnersNotLocked)]
    pub winners_ledger: Account<'info, WinnersLedger>,
    /// CHECK: Fixed service-fee recipient; no signature is required.
    #[account(mut, constraint = authority.key() == giveaway.provider_authority @ GiveawayError::Unauthorized)]
    pub authority: AccountInfo<'info>,
    /// CHECK: Fixed remainder recipient.
    #[account(mut, constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch)]
    pub creator: AccountInfo<'info>,
}

pub fn process<'info>(
    ctx: Context<'_, '_, '_, 'info, SettlePayoutBatch<'info>>,
    batch_index: u32,
    winners: Vec<WinnerProof>,
) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let vault = &ctx.accounts.vault;
    let root = &mut ctx.accounts.winners_ledger;
    let clock = Clock::get()?;
    if giveaway.settled {
        return Ok(());
    }
    require!(
        !winners.is_empty() && winners.len() <= MAX_WINNERS_PER_BATCH,
        GiveawayError::InvalidBatch
    );
    require!(
        root.winners_count > 0,
        GiveawayError::NoEligibleParticipants
    );
    require!(
        ctx.remaining_accounts.len() >= winners.len() * 2,
        GiveawayError::InvalidAccount
    );

    root.start_settlement(clock.unix_timestamp);
    giveaway.begin_settlement(clock.unix_timestamp);

    for (i, proof) in winners.iter().enumerate() {
        require!(
            root.validate_winner_index(proof.winner_index),
            GiveawayError::InvalidBatch
        );
        require_eq!(
            proof.amount,
            root.per_winner_lamports,
            GiveawayError::InvalidBatch
        );
        require!(
            proof.amount >= MIN_WINNER_PAYOUT_LAMPORTS,
            GiveawayError::InvalidBatch
        );
        let participant_info = &ctx.remaining_accounts[i * 2];
        let winner_info = &ctx.remaining_accounts[i * 2 + 1];
        require!(
            participant_info.is_writable && winner_info.is_writable,
            GiveawayError::AccountNotWritable
        );
        require_eq!(
            participant_info.owner,
            ctx.program_id,
            GiveawayError::InvalidProgram
        );

        let mut participant = {
            let account_data = participant_info.data.borrow();
            let mut participant_data = &account_data[..];
            Participant::try_deserialize(&mut participant_data)
                .map_err(|_| GiveawayError::AccountNotInitialized)?
        };
        require_keys_eq!(
            participant.giveaway,
            giveaway.key(),
            GiveawayError::InvalidAccount
        );
        require_keys_eq!(
            participant.wallet,
            proof.winner,
            GiveawayError::InvalidAccount
        );
        require_keys_eq!(
            winner_info.key(),
            proof.winner,
            GiveawayError::InvalidAccount
        );
        assert_pda_owned(
            ctx.program_id,
            participant_info,
            &[
                PARTICIPANT_SEED,
                giveaway.key().as_ref(),
                participant.wallet.as_ref(),
            ],
        )?;
        require!(
            participant.is_eligible(),
            GiveawayError::NoEligibleParticipants
        );
        let key = compute_candidate_key(&root_seed(root), &participant.wallet);
        require!(key <= root.threshold_key, GiveawayError::InvalidBatch);

        if participant.payout_version() == root.recompute_version {
            continue;
        }

        transfer_lamports(vault, winner_info, proof.amount)?;
        participant.mark_paid_for_version(root.recompute_version, clock.unix_timestamp);
        let mut account_data = participant_info.data.borrow_mut();
        account_data.fill(0);
        participant.try_serialize(&mut Cursor::new(&mut account_data[..]))?;
        root.mark_winner_paid()?;

        crate::events::GiveawayEvent::WinnerPaid {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            winner: proof.winner.to_string(),
            amount_lamports: proof.amount,
            batch_index,
            winner_index: proof.winner_index,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }
    root.mark_batch_processed()?;

    if root.is_settlement_complete() {
        root.complete_settlement(clock.unix_timestamp);
        let service_fee = giveaway.calculate_service_fee();
        if service_fee > 0 {
            transfer_lamports(vault, &ctx.accounts.authority, service_fee)?;
            crate::events::GiveawayEvent::ServiceFeePaid {
                giveaway_id: giveaway.id,
                giveaway: giveaway.key().to_string(),
                service_fee_lamports: service_fee,
                authority: ctx.accounts.authority.key().to_string(),
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }
        let refund = vault.lamports();
        if refund > 0 {
            transfer_lamports(vault, &ctx.accounts.creator, refund)?;
            crate::events::GiveawayEvent::CreatorRefunded {
                giveaway_id: giveaway.id,
                giveaway: giveaway.key().to_string(),
                creator: ctx.accounts.creator.key().to_string(),
                amount_lamports: refund,
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }
        giveaway.settle();
        crate::events::GiveawayEvent::GiveawaySettled {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            total_winners: root.winners_count,
            total_paid_lamports: root.total_payout_lamports,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }
    Ok(())
}

fn root_seed(root: &WinnersLedger) -> [u8; 32] {
    // The first half of every threshold key is H(domain || seed || wallet), so
    // payout verification needs the immutable seed. Protocol v2 stores it in
    // the first 32 reserved bytes to keep the settlement root fixed-size.
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&root.reserved[..32]);
    seed
}
