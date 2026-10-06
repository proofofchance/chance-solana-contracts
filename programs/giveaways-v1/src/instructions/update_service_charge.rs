//! # Update Service Charge Instruction
//!
//! Admin-only instruction to update the global service fee rate.

use crate::{error::GiveawayError, state::Config, utils::validation::validate_service_fee_bps};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct UpdateServiceCharge<'info> {
    #[account(
        mut,
        constraint = config.authority == authority.key() @ GiveawayError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account()]
    pub authority: Signer<'info>,
}

pub fn process(ctx: Context<UpdateServiceCharge>, new_service_fee_bps: u16) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let authority = &ctx.accounts.authority;
    let clock = Clock::get()?;

    // Validate new service fee
    validate_service_fee_bps(new_service_fee_bps)?;

    let old_fee = config.service_fee_bps;

    // Update service fee
    config.update_service_fee(new_service_fee_bps, clock.unix_timestamp);

    msg!(
        "Service fee updated from {} to {} basis points by {}",
        old_fee,
        new_service_fee_bps,
        authority.key()
    );

    Ok(())
}
