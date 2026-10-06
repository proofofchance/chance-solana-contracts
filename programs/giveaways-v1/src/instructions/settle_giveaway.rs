//! # Settle Giveaway Instruction
//!
//! Refund-only settlement for giveaway paths that never need a winners ledger:
//! - Zero participants
//! - No attesters
//! - Omitted accepted reveals after remediation expires

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
    utils::account::transfer_lamports,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct SettleGiveaway<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount,
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault account validated by constraint
    #[account(
        mut,
        constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount
    )]
    pub vault: AccountInfo<'info>,

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

pub fn process(ctx: Context<SettleGiveaway>) -> Result<()> {
    let accounts = ctx.accounts;
    let _config = &accounts.config;
    let giveaway_key = accounts.giveaway.key();
    let giveaway = &mut accounts.giveaway;
    let vault = &accounts.vault;
    let creator = &accounts.creator;
    let _authority = &accounts.authority;
    let clock = Clock::get()?;

    if giveaway.settled {
        return Ok(());
    }

    let reason = refund_reason(giveaway, clock.unix_timestamp)?;

    crate::events::GiveawayEvent::NoWinners {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        reason: reason.to_string(),
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
        "Giveaway {} settled via {} refund path",
        giveaway.id,
        reason
    );

    Ok(())
}

fn refund_reason(giveaway: &Giveaway, current_time: i64) -> Result<&'static str> {
    require!(!giveaway.winners_locked, GiveawayError::WinnersLocked);
    if giveaway.settlement_deadline_unix > 0 && current_time >= giveaway.settlement_deadline_unix {
        return Ok("finalization_timeout");
    }
    if giveaway.participants_count == 0 {
        require!(
            current_time >= giveaway.active_deadline_unix,
            GiveawayError::SettlementNotReady
        );
        Ok("no_participants")
    } else if giveaway.attested_count == 0 {
        require!(
            current_time >= giveaway.upload_deadline_unix,
            GiveawayError::SettlementNotReady
        );
        Ok("no_attesters")
    } else if giveaway.remediation_expired(current_time) {
        Ok("accepted_reveals_omitted_after_remediation")
    } else {
        Err(GiveawayError::InvalidInstruction.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{GiveawayStatus, GIVEAWAY_ACCOUNT_VERSION};

    fn test_giveaway() -> Giveaway {
        Giveaway {
            id: 1,
            config: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            vault: Pubkey::new_unique(),
            status: GiveawayStatus::Active,
            total_payout_lamports: 1_000_000,
            number_of_winners: 10,
            service_fee_bps: 500,
            created_at_unix: 0,
            active_start_unix: 0,
            active_deadline_unix: 100,
            upload_start_unix: 100,
            upload_deadline_unix: 200,
            settlement_start_unix: 0,
            participants_count: 0,
            attested_count: 0,
            provider_uploaded_count: 0,
            poc_aggregate_hash: [0; 32],
            uploads_complete: false,
            disqualified_count: 0,
            winners_computed: false,
            winners_locked: false,
            recompute_version: 0,
            settled: false,
            remediation_start_unix: 0,
            remediation_deadline_unix: 0,
            account_version: GIVEAWAY_ACCOUNT_VERSION,
            provider_authority: Pubkey::new_unique(),
            settlement_deadline_unix: i64::MAX,
            reserved: [0; 70],
        }
    }

    #[test]
    fn partial_finalization_times_out_but_vested_winners_never_refund() {
        let mut giveaway = test_giveaway();
        giveaway.participants_count = 3;
        giveaway.attested_count = 3;
        giveaway.provider_uploaded_count = 3;
        giveaway.uploads_complete = true;
        giveaway.winners_computed = true;
        giveaway.settlement_deadline_unix = 500;
        assert!(refund_reason(&giveaway, 499).is_err());
        assert_eq!(
            refund_reason(&giveaway, 500).unwrap(),
            "finalization_timeout"
        );
        giveaway.winners_locked = true;
        assert!(refund_reason(&giveaway, 500).is_err());
        assert!(refund_reason(&giveaway, i64::MAX).is_err());
    }

    #[test]
    fn refund_reason_accepts_simple_refund_paths() {
        let mut giveaway = test_giveaway();
        assert_eq!(refund_reason(&giveaway, 100).unwrap(), "no_participants");

        giveaway.participants_count = 3;
        assert_eq!(refund_reason(&giveaway, 200).unwrap(), "no_attesters");

        giveaway.attested_count = 2;
        giveaway.provider_uploaded_count = 1;
        giveaway.remediation_start_unix = 201;
        giveaway.remediation_deadline_unix = 301;
        assert_eq!(
            refund_reason(&giveaway, 302).unwrap(),
            "accepted_reveals_omitted_after_remediation"
        );
    }

    #[test]
    fn refund_reason_rejects_no_eligible_finalization_path() {
        let mut giveaway = test_giveaway();
        giveaway.participants_count = 3;
        giveaway.attested_count = 3;
        giveaway.provider_uploaded_count = 3;
        giveaway.uploads_complete = true;
        giveaway.winners_computed = true;

        assert!(refund_reason(&giveaway, 250).is_err());
    }
}
