//! # Account Utilities
//!
//! Utilities for reading and writing account data safely.

use std::io::Cursor;

use crate::error::GiveawayError;
use anchor_lang::prelude::*;

/// Read account data and deserialize
pub fn read_account_data<T: AccountDeserialize>(account: &AccountInfo) -> Result<T> {
    let mut data = &account.data.borrow()[..];
    T::try_deserialize(&mut data).map_err(|_| GiveawayError::AccountNotInitialized.into())
}

/// Write account data after serialization
pub fn write_account_data<T: AccountSerialize>(
    account: &AccountInfo,
    account_name: &str,
    data: &T,
) -> Result<()> {
    let mut serialized = Vec::new();
    data.try_serialize(&mut serialized).map_err(|_| {
        msg!("Failed to serialize {}", account_name);
        GiveawayError::InvalidInstruction
    })?;

    let mut account_data = account.data.borrow_mut();
    require!(
        serialized.len() <= account_data.len(),
        GiveawayError::InvalidInstruction
    );

    account_data.fill(0);
    let mut writer = Cursor::new(&mut account_data[..]);
    data.try_serialize(&mut writer).map_err(|_| {
        msg!("Failed to serialize {}", account_name);
        GiveawayError::InvalidInstruction
    })?;
    Ok(())
}

/// Transfer lamports between accounts
pub fn transfer_lamports(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    require!(from.lamports() >= amount, GiveawayError::InsufficientFunds);

    **from.try_borrow_mut_lamports()? = from
        .lamports()
        .checked_sub(amount)
        .ok_or(GiveawayError::MathOverflow)?;

    **to.try_borrow_mut_lamports()? = to
        .lamports()
        .checked_add(amount)
        .ok_or(GiveawayError::MathOverflow)?;

    Ok(())
}

/// Create account with PDA
pub fn create_account_with_pda<'info>(
    payer: &AccountInfo<'info>,
    account: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    program_id: &Pubkey,
    seeds: &[&[u8]],
    bump: u8,
    space: usize,
) -> Result<()> {
    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(space);

    let bump_arr = [bump];
    let bump_ref: &[u8] = &bump_arr;
    let mut seeds_with_bump: Vec<&[u8]> = seeds.to_vec();
    seeds_with_bump.push(bump_ref);

    anchor_lang::solana_program::program::invoke_signed(
        &anchor_lang::solana_program::system_instruction::create_account(
            payer.key,
            account.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[payer.clone(), account.clone(), system_program.clone()],
        &[&seeds_with_bump],
    )?;

    Ok(())
}
