use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use chance_registry_wire as wire;
use sha2::{Digest, Sha256};

#[derive(Accounts)]
pub struct BindRegistry<'info> {
    #[account(mut, seeds = [b"config"], bump, has_one = authority)]
    pub config: Account<'info, Config>,
    pub authority: Signer<'info>,
    /// CHECK: Executable, ownership, PDA, schema and release checks in handler.
    pub registry_program: UncheckedAccount<'info>,
    /// CHECK: Validated with registry program owner and canonical seeds.
    pub registry_config: UncheckedAccount<'info>,
    /// CHECK: Validated program release PDA and domain.
    pub release: UncheckedAccount<'info>,
}

pub fn bind(ctx: Context<BindRegistry>) -> Result<()> {
    require!(
        ctx.accounts.config.registry_program == Pubkey::default()
            && ctx.accounts.config.next_giveaway_id == 1,
        GiveawayError::InvalidInstruction
    );
    validate_registry(
        ctx.program_id,
        &ctx.accounts.registry_program.to_account_info(),
        &ctx.accounts.registry_config.to_account_info(),
        &ctx.accounts.release.to_account_info(),
    )?;
    ctx.accounts.config.registry_program = ctx.accounts.registry_program.key();
    ctx.accounts.config.registry_config = ctx.accounts.registry_config.key();
    Ok(())
}
fn validate_registry(
    program_id: &Pubkey,
    registry_program: &AccountInfo,
    cfg: &AccountInfo,
    release: &AccountInfo,
) -> Result<()> {
    require!(
        registry_program.executable
            && cfg.owner == registry_program.key
            && release.owner == registry_program.key,
        GiveawayError::InvalidInstruction
    );
    let config: wire::Config = wire::from_slice(&cfg.try_borrow_data()?)
        .map_err(|_| error!(GiveawayError::InvalidInstruction))?;
    let rel: wire::Release = wire::from_slice(&release.try_borrow_data()?)
        .map_err(|_| error!(GiveawayError::InvalidInstruction))?;
    require!(
        config.tag == wire::CONFIG_TAG
            && config.schema == wire::SCHEMA
            && rel.tag == wire::RELEASE_TAG
            && rel.domain == wire::GIVEAWAY
            && rel.program == program_id.to_bytes()
            && rel.registry == cfg.key.to_bytes()
            && Pubkey::find_program_address(
                &[b"registry", &config.authority],
                registry_program.key
            )
            .0 == *cfg.key
            && Pubkey::find_program_address(
                &[b"release", cfg.key.as_ref(), program_id.as_ref()],
                registry_program.key
            )
            .0 == *release.key,
        GiveawayError::InvalidInstruction
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn record<'a>(
    program_id: &Pubkey,
    config: &Config,
    giveaway_ai: &AccountInfo<'a>,
    creator: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    registry_program: &AccountInfo<'a>,
    cfg: &AccountInfo<'a>,
    release: &AccountInfo<'a>,
    signer: &AccountInfo<'a>,
    entry: &AccountInfo<'a>,
    business: &AccountInfo<'a>,
    giveaway: &Giveaway,
    nonce: u64,
) -> Result<()> {
    require!(
        *registry_program.key == config.registry_program && *cfg.key == config.registry_config,
        GiveawayError::InvalidInstruction
    );
    validate_registry(program_id, registry_program, cfg, release)?;
    let (expected, bump) =
        Pubkey::find_program_address(&[b"chance-release", cfg.key.as_ref()], program_id);
    require!(*signer.key == expected, GiveawayError::InvalidInstruction);
    let rules_hash: [u8; 32] = Sha256::digest(giveaway.try_to_vec()?).into();
    let data = wire::to_vec(&wire::Instruction::RecordInstance {
        day_or_nonce: nonce,
        rules_hash,
    })
    .map_err(|_| error!(GiveawayError::InvalidInstruction))?;
    let ix = Instruction {
        program_id: *registry_program.key,
        data,
        accounts: vec![
            AccountMeta::new(*cfg.key, false),
            AccountMeta::new_readonly(*release.key, false),
            AccountMeta::new_readonly(*signer.key, true),
            AccountMeta::new_readonly(*giveaway_ai.key, false),
            AccountMeta::new_readonly(*creator.key, true),
            AccountMeta::new(*creator.key, true),
            AccountMeta::new_readonly(*system.key, false),
            AccountMeta::new(*entry.key, false),
            AccountMeta::new(*business.key, false),
        ],
    };
    invoke_signed(
        &ix,
        &[
            cfg.clone(),
            release.clone(),
            signer.clone(),
            giveaway_ai.clone(),
            creator.clone(),
            system.clone(),
            entry.clone(),
            business.clone(),
            registry_program.clone(),
        ],
        &[&[b"chance-release", cfg.key.as_ref(), &[bump]]],
    )?;
    Ok(())
}
