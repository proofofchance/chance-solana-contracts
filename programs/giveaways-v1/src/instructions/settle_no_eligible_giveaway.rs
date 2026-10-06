//! # Settle No-Eligible Giveaway Instruction
//!
//! Refunds a giveaway after finalization proves there are zero eligible winners.
//! Unlike refund-only settlement, this path requires an existing winners ledger
//! because the ledger is the on-chain proof that finalization completed with no
//! payable winners.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway, WinnersLedger},
    utils::account::transfer_lamports,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct SettleNoEligibleGiveaway<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount,
        constraint = giveaway.winners_computed @ GiveawayError::WinnersNotComputed,
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault account validated by constraint
    #[account(
        mut,
        constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount
    )]
    pub vault: AccountInfo<'info>,

    #[account(
        constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = winners_ledger.winners_count == 0 @ GiveawayError::InvalidInstruction,
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    /// CHECK: Fixed refund recipient is constrained to the instance creator.
    #[account(
        mut,
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: AccountInfo<'info>,

    /// CHECK: Authority account - can be same as creator
    #[account(
        constraint = authority.key() == giveaway.provider_authority || authority.key() == giveaway.creator @ GiveawayError::Unauthorized
    )]
    pub authority: AccountInfo<'info>,
}

pub fn process(ctx: Context<SettleNoEligibleGiveaway>) -> Result<()> {
    let accounts = ctx.accounts;
    let giveaway_key = accounts.giveaway.key();
    let giveaway = &mut accounts.giveaway;
    let vault = &accounts.vault;
    let creator = &accounts.creator;
    let _authority = &accounts.authority;
    let clock = Clock::get()?;

    if giveaway.settled {
        return Ok(());
    }

    crate::events::GiveawayEvent::NoWinners {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        reason: "no_eligible_participants".to_string(),
        total_participants: giveaway.participants_count,
        total_attested: giveaway.attested_count,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    let refund = vault.lamports();
    if refund > 0 {
        transfer_lamports(vault, creator, refund)?;

        crate::events::GiveawayEvent::CreatorRefunded {
            giveaway_id: giveaway.id,
            giveaway: giveaway_key.to_string(),
            creator: creator.key().to_string(),
            amount_lamports: refund,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    giveaway.settle();

    crate::events::GiveawayEvent::GiveawaySettled {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        total_winners: 0,
        total_paid_lamports: 0,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Giveaway {} settled via no_eligible_participants refund path",
        giveaway.id
    );

    Ok(())
}
