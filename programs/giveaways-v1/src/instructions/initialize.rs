//! # Initialize Instruction
//!
//! Sets up the global configuration for the giveaways program.
//! Can only be called once per deployment.

use crate::{constants::*, state::Config, utils::validation::*};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = Config::SIZE,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, Config>,

    #[account(mut, constraint = initialization_authorized(authority.key()) @ crate::error::GiveawayError::Unauthorized)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

fn initialization_authorized(authority: Pubkey) -> bool {
    #[cfg(feature = "devnet-v1")]
    {
        authority == crate::INITIALIZATION_AUTHORITY
    }
    #[cfg(not(feature = "devnet-v1"))]
    {
        let _ = authority;
        true
    }
}

#[cfg(all(test, feature = "devnet-v1"))]
mod tests {
    use super::*;

    #[test]
    fn release_bootstrap_authority_is_fixed_without_restricting_creators() {
        assert!(initialization_authorized(crate::INITIALIZATION_AUTHORITY));
        assert!(!initialization_authorized(Pubkey::new_unique()));
    }
}

pub fn process(
    ctx: Context<Initialize>,
    service_fee_bps: u16,
    default_active_duration_secs: u32,
    default_upload_duration_secs: u32,
) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let authority = &ctx.accounts.authority;
    let clock = Clock::get()?;

    // Validate parameters
    validate_service_fee_bps(service_fee_bps)?;
    validate_duration(
        default_active_duration_secs,
        MIN_ACTIVE_DURATION_SECS,
        MAX_ACTIVE_DURATION_SECS,
    )?;
    validate_duration(
        default_upload_duration_secs,
        MIN_UPLOAD_DURATION_SECS,
        MAX_UPLOAD_DURATION_SECS,
    )?;

    // Initialize config
    config.initialize(
        authority.key(),
        service_fee_bps,
        default_active_duration_secs,
        default_upload_duration_secs,
        clock.unix_timestamp,
    );

    msg!(
        "Giveaways program initialized with authority: {}, service_fee_bps: {}, defaults: {}s active, {}s upload",
        authority.key(),
        service_fee_bps,
        default_active_duration_secs,
        default_upload_duration_secs
    );

    Ok(())
}
