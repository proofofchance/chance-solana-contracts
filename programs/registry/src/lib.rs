#![allow(unexpected_cfgs)]
//! Registry schema 1. Lifecycle only gates creation; it cannot move old escrow.
use borsh::{BorshDeserialize, BorshSerialize};
use chance_registry_wire as wire;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};
use solana_sha256_hasher::hashv;
use solana_system_interface::{instruction as system_instruction, program as system_program};

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Error {
    Invalid = 1,
    Unauthorized,
    WrongState,
    Duplicate,
    MutableProgram,
    Delay,
    Overflow,
}
impl From<Error> for ProgramError {
    fn from(value: Error) -> Self {
        ProgramError::Custom(value as u32)
    }
}
fn require(ok: bool, err: Error) -> ProgramResult {
    if ok {
        Ok(())
    } else {
        Err(err.into())
    }
}
fn checked_next(value: u64) -> Result<u64, ProgramError> {
    value.checked_add(1).ok_or(Error::Overflow.into())
}
fn read<T: BorshDeserialize>(
    ai: &AccountInfo,
    owner: &Pubkey,
    tag: [u8; 8],
) -> Result<T, ProgramError> {
    require(ai.owner == owner, Error::Invalid)?;
    let data = ai.try_borrow_data()?;
    require(data.starts_with(&tag), Error::Invalid)?;
    T::try_from_slice(&data).map_err(|_| Error::Invalid.into())
}
fn write<T: BorshSerialize>(ai: &AccountInfo, data: &T) -> ProgramResult {
    require(ai.is_writable, Error::Invalid)?;
    data.serialize(&mut &mut ai.try_borrow_mut_data()?[..])
        .map_err(|_| Error::Invalid.into())
}
fn key(program: &Pubkey, ai: &AccountInfo, seeds: &[&[u8]]) -> Result<u8, ProgramError> {
    let (expected, bump) = Pubkey::find_program_address(seeds, program);
    require(expected == *ai.key, Error::Invalid)?;
    Ok(bump)
}
fn create<'a, T: BorshSerialize>(
    program: &Pubkey,
    ai: &AccountInfo<'a>,
    payer: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    seeds: &[&[u8]],
    data: &T,
) -> ProgramResult {
    require(
        payer.is_signer && payer.is_writable && ai.is_writable,
        Error::Unauthorized,
    )?;
    require(*system.key == system_program::id(), Error::Invalid)?;
    require(
        ai.owner == &system_program::id() && ai.data_is_empty(),
        Error::Duplicate,
    )?;
    let bump = [key(program, ai, seeds)?];
    let mut signing_seeds = seeds.to_vec();
    signing_seeds.push(&bump);
    let bytes = borsh::to_vec(data).map_err(|_| Error::Invalid)?;
    let rent = Rent::get()?.minimum_balance(bytes.len());
    if ai.lamports() == 0 {
        invoke_signed(
            &system_instruction::create_account(
                payer.key,
                ai.key,
                rent,
                bytes.len() as u64,
                program,
            ),
            &[payer.clone(), ai.clone(), system.clone()],
            &[&signing_seeds],
        )?;
    } else {
        // An unsolicited lamport donation must not squat a canonical business key.
        let missing = rent.saturating_sub(ai.lamports());
        if missing != 0 {
            invoke(
                &system_instruction::transfer(payer.key, ai.key, missing),
                &[payer.clone(), ai.clone(), system.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(ai.key, bytes.len() as u64),
            &[ai.clone(), system.clone()],
            &[&signing_seeds],
        )?;
        invoke_signed(
            &system_instruction::assign(ai.key, program),
            &[ai.clone(), system.clone()],
            &[&signing_seeds],
        )?;
    }
    write(ai, data)
}
fn config(program: &Pubkey, ai: &AccountInfo) -> Result<wire::Config, ProgramError> {
    let cfg: wire::Config = read(ai, program, wire::CONFIG_TAG)?;
    require(cfg.schema == wire::SCHEMA, Error::Invalid)?;
    key(program, ai, &[b"registry", &cfg.authority])?;
    Ok(cfg)
}
fn release(
    program: &Pubkey,
    cfg: &AccountInfo,
    ai: &AccountInfo,
) -> Result<wire::Release, ProgramError> {
    let rel: wire::Release = read(ai, program, wire::RELEASE_TAG)?;
    require(rel.registry == cfg.key.to_bytes(), Error::Invalid)?;
    key(program, ai, &[b"release", cfg.key.as_ref(), &rel.program])?;
    Ok(rel)
}
fn authorize(cfg: &wire::Config, actor: &AccountInfo, guardian: bool) -> ProgramResult {
    require(
        actor.is_signer
            && (actor.key.to_bytes() == cfg.authority
                || (guardian && actor.key.to_bytes() == cfg.guardian)),
        Error::Unauthorized,
    )
}

/// BPF upgradeable-loader state has fixed bincode offsets. Only immutable ProgramData
/// is accepted; no development escape hatch exists in the activation instruction.
fn require_immutable(program: &AccountInfo, data: &AccountInfo) -> ProgramResult {
    let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
    require(
        program.executable && *program.owner == loader && *data.owner == loader,
        Error::MutableProgram,
    )?;
    let p = program.try_borrow_data()?;
    let d = data.try_borrow_data()?;
    require(
        p.len() == 36 && p[..4] == 2u32.to_le_bytes() && p[4..36] == data.key.to_bytes(),
        Error::MutableProgram,
    )?;
    key(&loader, data, &[program.key.as_ref()])?;
    require(
        d.len() > 45 && d[..4] == 3u32.to_le_bytes() && d[12] == 0,
        Error::MutableProgram,
    )
}
fn validate_reason(reason: &str) -> ProgramResult {
    require(
        !reason.trim().is_empty() && reason.len() <= wire::MAX_REASON,
        Error::Invalid,
    )
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    bytes: &[u8],
) -> ProgramResult {
    let instruction: wire::Instruction = wire::from_slice(bytes).map_err(|_| Error::Invalid)?;
    let iter = &mut accounts.iter();
    let cfg_ai = next_account_info(iter)?;
    if let wire::Instruction::Initialize {
        guardian,
        activation_delay,
    } = instruction
    {
        let authority = next_account_info(iter)?;
        let system = next_account_info(iter)?;
        require(guardian != [0; 32] && activation_delay > 0, Error::Invalid)?;
        return create(
            program_id,
            cfg_ai,
            authority,
            system,
            &[b"registry", authority.key.as_ref()],
            &wire::Config {
                tag: wire::CONFIG_TAG,
                schema: wire::SCHEMA,
                authority: authority.key.to_bytes(),
                guardian,
                activation_delay,
                release_count: 0,
                record_count: 0,
                instance_count: 0,
            },
        );
    }
    let mut cfg = config(program_id, cfg_ai)?;
    let rel_ai = next_account_info(iter)?;
    let actor = next_account_info(iter)?;
    if let wire::Instruction::Register {
        domain,
        series,
        source_hash,
        executable_hash,
    } = instruction
    {
        let system = next_account_info(iter)?;
        let target = next_account_info(iter)?;
        authorize(&cfg, actor, false)?;
        require(
            target.executable
                && (domain == wire::DAILY || domain == wire::GIVEAWAY)
                && series != [0; 32]
                && source_hash != [0; 32]
                && executable_hash != [0; 32],
            Error::Invalid,
        )?;
        let clock = Clock::get()?;
        cfg.release_count = checked_next(cfg.release_count)?;
        create(
            program_id,
            rel_ai,
            actor,
            system,
            &[b"release", cfg_ai.key.as_ref(), target.key.as_ref()],
            &wire::Release {
                tag: wire::RELEASE_TAG,
                registry: cfg_ai.key.to_bytes(),
                program: target.key.to_bytes(),
                domain,
                series,
                source_hash,
                executable_hash,
                sequence: cfg.release_count,
                registered_slot: clock.slot,
                eligible_at: clock
                    .unix_timestamp
                    .checked_add(i64::from(cfg.activation_delay))
                    .ok_or(Error::Overflow)?,
                status: wire::REGISTERED,
                successor: [0; 32],
            },
        )?;
        return write(cfg_ai, &cfg);
    }
    let mut rel = release(program_id, cfg_ai, rel_ai)?;
    if let wire::Instruction::RecordInstance {
        day_or_nonce,
        rules_hash,
    } = instruction
    {
        let instance = next_account_info(iter)?;
        let creator = next_account_info(iter)?;
        let payer = next_account_info(iter)?;
        let system = next_account_info(iter)?;
        let entry = next_account_info(iter)?;
        let business = next_account_info(iter)?;
        require(rel.status == wire::ACTIVE, Error::WrongState)?;
        let owner_program = Pubkey::new_from_array(rel.program);
        key(
            &owner_program,
            actor,
            &[b"chance-release", cfg_ai.key.as_ref()],
        )?;
        require(
            actor.is_signer
                && creator.is_signer
                && *instance.owner == owner_program
                && !instance.data_is_empty()
                && rules_hash != [0; 32],
            Error::Unauthorized,
        )?;
        let clock = Clock::get()?;
        let business_key = business_key(&rel, creator.key.as_ref(), day_or_nonce);
        cfg.instance_count = checked_next(cfg.instance_count)?;
        let seq = cfg.instance_count.to_le_bytes();
        let record = wire::Instance {
            tag: wire::INSTANCE_TAG,
            registry: cfg_ai.key.to_bytes(),
            sequence: cfg.instance_count,
            release: rel_ai.key.to_bytes(),
            owner_program: rel.program,
            instance: instance.key.to_bytes(),
            creator: creator.key.to_bytes(),
            business_key,
            rules_hash,
            created_slot: clock.slot,
        };
        create(
            program_id,
            business,
            payer,
            system,
            &[
                b"business",
                cfg_ai.key.as_ref(),
                &[rel.domain],
                &business_key,
            ],
            &wire::BusinessKey {
                tag: wire::KEY_TAG,
                entry: entry.key.to_bytes(),
            },
        )?;
        create(
            program_id,
            entry,
            payer,
            system,
            &[b"instance", cfg_ai.key.as_ref(), &seq],
            &record,
        )?;
        write(cfg_ai, &cfg)?;
        solana_program::log::sol_log_data(&[&borsh::to_vec(&record).map_err(|_| Error::Invalid)?]);
        return Ok(());
    }
    let system = next_account_info(iter)?;
    let record_ai = next_account_info(iter)?;
    let clock = Clock::get()?;
    let reason = match instruction {
        wire::Instruction::Activate { reason } => {
            authorize(&cfg, actor, false)?;
            require(
                rel.status == wire::REGISTERED || rel.status == wire::PAUSED,
                Error::WrongState,
            )?;
            require(clock.unix_timestamp >= rel.eligible_at, Error::Delay)?;
            let target = next_account_info(iter)?;
            let target_data = next_account_info(iter)?;
            let registry_program = next_account_info(iter)?;
            let registry_data = next_account_info(iter)?;
            require(
                target.key.to_bytes() == rel.program && registry_program.key == program_id,
                Error::Invalid,
            )?;
            require_immutable(target, target_data)?;
            let binary = target_data.try_borrow_data()?;
            let payload = &binary[45..];
            let end = payload
                .iter()
                .rposition(|byte| *byte != 0)
                .map_or(0, |i| i + 1);
            require(
                hashv(&[&payload[..end]]).to_bytes() == rel.executable_hash,
                Error::Invalid,
            )?;
            require_immutable(registry_program, registry_data)?;
            rel.status = wire::ACTIVE;
            reason
        }
        wire::Instruction::Pause { reason } => {
            authorize(&cfg, actor, true)?;
            require(rel.status == wire::ACTIVE, Error::WrongState)?;
            rel.status = wire::PAUSED;
            rel.eligible_at = clock
                .unix_timestamp
                .checked_add(i64::from(cfg.activation_delay))
                .ok_or(Error::Overflow)?;
            reason
        }
        wire::Instruction::Discontinue { reason } => {
            authorize(&cfg, actor, true)?;
            require(rel.status != wire::DISCONTINUED, Error::WrongState)?;
            rel.status = wire::DISCONTINUED;
            reason
        }
        wire::Instruction::SetSuccessor { reason } => {
            authorize(&cfg, actor, false)?;
            require(rel.status == wire::DISCONTINUED, Error::WrongState)?;
            let next_ai = next_account_info(iter)?;
            let next = release(program_id, cfg_ai, next_ai)?;
            require(
                next.sequence > rel.sequence
                    && next.domain == rel.domain
                    && next.series == rel.series,
                Error::Invalid,
            )?;
            rel.successor = next_ai.key.to_bytes();
            reason
        }
        _ => return Err(Error::Invalid.into()),
    };
    validate_reason(&reason)?;
    cfg.record_count = checked_next(cfg.record_count)?;
    let record = wire::Record {
        tag: wire::RECORD_TAG,
        registry: cfg_ai.key.to_bytes(),
        sequence: cfg.record_count,
        release: rel_ai.key.to_bytes(),
        actor: actor.key.to_bytes(),
        slot: clock.slot,
        timestamp: clock.unix_timestamp,
        status: rel.status,
        successor: rel.successor,
        reason,
    };
    create(
        program_id,
        record_ai,
        actor,
        system,
        &[
            b"record",
            cfg_ai.key.as_ref(),
            &cfg.record_count.to_le_bytes(),
        ],
        &record,
    )?;
    write(rel_ai, &rel)?;
    write(cfg_ai, &cfg)?;
    solana_program::log::sol_log_data(&[&borsh::to_vec(&record).map_err(|_| Error::Invalid)?]);
    Ok(())
}

pub fn business_key(release: &wire::Release, creator: &[u8], day_or_nonce: u64) -> [u8; 32] {
    let index = day_or_nonce.to_le_bytes();
    if release.domain == wire::DAILY {
        hashv(&[b"chance-daily-key-v1", &release.series, &index]).to_bytes()
    } else {
        hashv(&[b"chance-giveaway-key-v1", creator, &index]).to_bytes()
    }
}
