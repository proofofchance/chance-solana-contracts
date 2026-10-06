use crate::{
    error::Error,
    state::{Config, Lottery},
    utils::{
        account::{read_account_data, write_account_data},
        pda::assert_pda_owned,
    },
};
use chance_registry_wire as wire;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    pubkey::Pubkey,
};

pub fn bind(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [config_ai, authority, registry_program, registry_config, release, ..] = accounts else {
        return Err(Error::MissingAccount.into());
    };
    assert_pda_owned(program_id, config_ai, &[b"config"])?;
    let mut config: Config = read_account_data(config_ai)?;
    if !authority.is_signer
        || *authority.key != config.authority
        || config.registry_program != Pubkey::default()
        || config.lottery_count != 0
    {
        return Err(Error::Unauthorized.into());
    }
    validate_registry(program_id, registry_program, registry_config, release)?;
    config.registry_program = *registry_program.key;
    config.registry_config = *registry_config.key;
    write_account_data(config_ai, "Config", &config)
}
fn validate_registry(
    program_id: &Pubkey,
    registry_program: &AccountInfo,
    cfg: &AccountInfo,
    release: &AccountInfo,
) -> ProgramResult {
    if !registry_program.executable
        || cfg.owner != registry_program.key
        || release.owner != registry_program.key
    {
        return Err(Error::IncorrectOwner.into());
    }
    let config: wire::Config =
        wire::from_slice(&cfg.try_borrow_data()?).map_err(|_| Error::InvalidAccountData)?;
    let rel: wire::Release =
        wire::from_slice(&release.try_borrow_data()?).map_err(|_| Error::InvalidAccountData)?;
    if config.tag != wire::CONFIG_TAG
        || config.schema != wire::SCHEMA
        || rel.tag != wire::RELEASE_TAG
        || rel.domain != wire::DAILY
        || rel.program != program_id.to_bytes()
        || rel.registry != cfg.key.to_bytes()
        || Pubkey::find_program_address(&[b"registry", &config.authority], registry_program.key).0
            != *cfg.key
        || Pubkey::find_program_address(
            &[b"release", cfg.key.as_ref(), program_id.as_ref()],
            registry_program.key,
        )
        .0 != *release.key
    {
        return Err(Error::InvalidAccountData.into());
    }
    Ok(())
}
/// Remaining creation accounts: registry executable, registry config, release,
/// release signer PDA, new sequential entry, canonical business-key account.
#[allow(clippy::too_many_arguments)]
pub fn record<'a>(
    program_id: &Pubkey,
    config: &Config,
    lottery_ai: &AccountInfo<'a>,
    creator: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    accounts: &[AccountInfo<'a>],
    lottery: &Lottery,
) -> ProgramResult {
    let [registry_program, cfg, release, signer, entry, business, ..] = accounts else {
        return Err(Error::MissingAccount.into());
    };
    if *registry_program.key != config.registry_program || *cfg.key != config.registry_config {
        return Err(Error::InvalidAccountData.into());
    }
    validate_registry(program_id, registry_program, cfg, release)?;
    let (expected, bump) =
        Pubkey::find_program_address(&[b"chance-release", cfg.key.as_ref()], program_id);
    if *signer.key != expected {
        return Err(Error::InvalidSeeds.into());
    }
    let rules_hash =
        solana_sha256_hasher::hash(&borsh::to_vec(lottery).map_err(|_| Error::InvalidAccountData)?)
            .to_bytes();
    let data = wire::to_vec(&wire::Instruction::RecordInstance {
        day_or_nonce: u64::try_from(lottery.buy_start_unix)
            .map_err(|_| Error::InvalidInstruction)?
            / 86_400,
        rules_hash,
    })
    .map_err(|_| Error::InvalidInstruction)?;
    let ix = Instruction {
        program_id: *registry_program.key,
        data,
        accounts: vec![
            AccountMeta::new(*cfg.key, false),
            AccountMeta::new_readonly(*release.key, false),
            AccountMeta::new_readonly(*signer.key, true),
            AccountMeta::new_readonly(*lottery_ai.key, false),
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
            lottery_ai.clone(),
            creator.clone(),
            system.clone(),
            entry.clone(),
            business.clone(),
            registry_program.clone(),
        ],
        &[&[b"chance-release", cfg.key.as_ref(), &[bump]]],
    )
}
