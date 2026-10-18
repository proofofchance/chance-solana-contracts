//! One-shot early upload for explicitly bound local/staging testing artifacts.
//! The production/default artifact contains only the rejecting entry point.
use crate::error::Error;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

#[cfg(not(feature = "allow-early-upload"))]
pub fn process(_program_id: &Pubkey, _accounts: &[AccountInfo]) -> ProgramResult {
    Err(Error::InvalidPhaseTransition.into())
}

#[cfg(feature = "allow-early-upload")]
pub fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    use crate::{
        events::LotteryEvent,
        state::{Config, Lottery},
        utils::{
            account::{read_account_data, validate_account_discriminator, write_account_data},
            pda::assert_pda_owned,
            validation::{require_signer, require_writable},
        },
    };
    use solana_program::{clock::Clock, sysvar::Sysvar};

    let [config_ai, lottery_ai, authority, registry_program, registry_config, release] = accounts
    else {
        return Err(Error::MissingAccount.into());
    };
    if *program_id != crate::TEST_CONTROL_PROGRAM_ID
        || *registry_program.key != crate::TEST_CONTROL_REGISTRY_ID
    {
        return Err(Error::Unauthorized.into());
    }
    require_signer(authority)?;
    require_writable(lottery_ai)?;
    assert_pda_owned(program_id, config_ai, &[b"config"])?;
    if !validate_account_discriminator(config_ai, "Config")
        || !validate_account_discriminator(lottery_ai, "Lottery")
    {
        return Err(Error::InvalidAccountData.into());
    }
    let config: Config = read_account_data(config_ai)?;
    let mut lottery: Lottery = read_account_data(lottery_ai)?;
    assert_pda_owned(
        program_id,
        lottery_ai,
        &[
            b"lottery",
            config_ai.key.as_ref(),
            &lottery.id.to_le_bytes(),
        ],
    )?;
    if lottery.config != *config_ai.key
        || config.registry_program != *registry_program.key
        || config.registry_config != *registry_config.key
    {
        return Err(Error::InvalidAccountData.into());
    }
    if lottery.authority != *authority.key || lottery.authority != crate::TEST_CONTROL_AUTHORITY {
        return Err(Error::Unauthorized.into());
    }
    super::registry::validate_registry(program_id, registry_program, registry_config, release)?;
    let old = [
        lottery.buy_deadline_unix,
        lottery.upload_start_unix,
        lottery.upload_deadline_unix,
        lottery.settlement_deadline_unix,
    ];
    let now = Clock::get()?.unix_timestamp;
    begin_upload(&mut lottery, now)?;
    let event = LotteryEvent::TestUploadPhaseBegan {
        capability: "begin-upload-v1".into(),
        lottery_id: lottery.id,
        lottery: lottery_ai.key.to_string(),
        config: config_ai.key.to_string(),
        authority: authority.key.to_string(),
        old_buy_deadline: old[0],
        old_upload_start: old[1],
        old_upload_deadline: old[2],
        old_settlement_deadline: old[3],
        new_buy_deadline: lottery.buy_deadline_unix,
        new_upload_start: lottery.upload_start_unix,
        new_upload_deadline: lottery.upload_deadline_unix,
        new_settlement_deadline: lottery.settlement_deadline_unix,
        timestamp: now,
    };
    write_account_data(lottery_ai, "Lottery", &lottery)?;
    event.emit();
    Ok(())
}

#[cfg(all(test, feature = "allow-early-upload"))]
mod tests {
    use super::*;
    use crate::state::Lottery;

    #[test]
    fn randomized_cutoffs_preserve_all_non_timing_state_and_never_repeat() {
        let mut seed = 0x123456789abcdef0u64;
        for _ in 0..512 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = 1 + (seed % 1_000_000_000) as i64;
            let mut lottery = Lottery::new(
                1,
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                start,
                Pubkey::new_unique(),
                255,
                500,
                1_000_000,
            );
            lottery.participants_count = seed % 256;
            lottery.total_tickets = lottery.participants_count * 3;
            lottery.total_funds = lottery.total_tickets * lottery.ticket_price_lamports;
            let original = lottery.clone();
            let now = start + (seed % 86400) as i64;
            begin_upload(&mut lottery, now).unwrap();
            let mut normalized = lottery.clone();
            normalized.buy_deadline_unix = original.buy_deadline_unix;
            normalized.upload_start_unix = original.upload_start_unix;
            normalized.upload_deadline_unix = original.upload_deadline_unix;
            normalized.settlement_deadline_unix = original.settlement_deadline_unix;
            normalized.reserved[0] = original.reserved[0];
            assert_eq!(
                borsh::to_vec(&normalized).unwrap(),
                borsh::to_vec(&original).unwrap()
            );
            let frozen = borsh::to_vec(&lottery).unwrap();
            for time in [
                start - 1,
                start,
                now,
                now + 1,
                original.buy_deadline_unix,
                i64::MAX,
            ] {
                assert_eq!(
                    begin_upload(&mut lottery, time),
                    Err(Error::InvalidPhaseTransition)
                );
                assert_eq!(borsh::to_vec(&lottery).unwrap(), frozen);
            }
        }
    }

    #[test]
    fn invalid_duration_or_overflow_is_atomic() {
        let mut lottery = Lottery::new(
            1,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            100,
            Pubkey::new_unique(),
            255,
            500,
            1_000_000,
        );
        lottery.upload_deadline_unix = i64::MAX;
        let original = borsh::to_vec(&lottery).unwrap();
        assert_eq!(begin_upload(&mut lottery, 101), Err(Error::MathOverflow));
        assert_eq!(borsh::to_vec(&lottery).unwrap(), original);
        lottery.upload_deadline_unix = lottery.upload_start_unix;
        let original = borsh::to_vec(&lottery).unwrap();
        assert_eq!(
            begin_upload(&mut lottery, 101),
            Err(Error::InvalidUploadWindow)
        );
        assert_eq!(borsh::to_vec(&lottery).unwrap(), original);
    }
}

#[cfg(feature = "allow-early-upload")]
fn begin_upload(lottery: &mut crate::state::Lottery, now: i64) -> Result<(), Error> {
    use crate::state::{
        lottery::LOTTERY_ACCOUNT_VERSION, sizes::DEFAULT_REMEDIATION_WINDOW_SECS, LotteryStatus,
    };
    if lottery.reserved[0] & 1 != 0
        || lottery.account_version != LOTTERY_ACCOUNT_VERSION
        || lottery.status != u8::from(LotteryStatus::Active)
        || lottery.settled
        || lottery.settlement_complete
        || !lottery.is_in_buy_window(now)
        || lottery.buy_deadline_unix != lottery.upload_start_unix
        || lottery.attested_count != 0
        || lottery.provider_uploaded_count != 0
        || lottery.uploads_complete
        || lottery.poc_aggregate_hash != [0; 32]
        || lottery.settlement_start_unix != 0
        || lottery.remediation_start_unix != 0
        || lottery.remediation_deadline_unix != 0
        || lottery.has_vested_winners()
        || lottery.settlement_batches_completed != 0
        || lottery.paid_winners_bitmap.iter().any(|byte| *byte != 0)
    {
        return Err(Error::InvalidPhaseTransition);
    }
    let duration = lottery
        .upload_deadline_unix
        .checked_sub(lottery.upload_start_unix)
        .filter(|value| *value > 0)
        .ok_or(Error::InvalidUploadWindow)?;
    let original_end = lottery
        .upload_deadline_unix
        .checked_add(2 * DEFAULT_REMEDIATION_WINDOW_SECS)
        .ok_or(Error::MathOverflow)?;
    if original_end != lottery.settlement_deadline_unix {
        return Err(Error::InvalidUploadWindow);
    }
    let upload_end = now.checked_add(duration).ok_or(Error::MathOverflow)?;
    let settlement_end = upload_end
        .checked_add(2 * DEFAULT_REMEDIATION_WINDOW_SECS)
        .ok_or(Error::MathOverflow)?;
    lottery.buy_deadline_unix = now;
    lottery.upload_start_unix = now;
    lottery.upload_deadline_unix = upload_end;
    lottery.settlement_deadline_unix = settlement_end;
    // A durable testing-only marker makes single use independent of clock
    // monotonicity. Layout V2 already reserves this byte; production never sets it.
    lottery.reserved[0] |= 1;
    Ok(())
}
