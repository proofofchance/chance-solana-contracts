mod common;

use borsh::BorshDeserialize;
use common::TestContext;
use daily_lottery::*;
use solana_instruction::{AccountMeta, Instruction as SdkIx};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::program as system_program;
use std::io::Cursor;

fn read_after_disc<T: BorshDeserialize>(data: &[u8]) -> T {
    let mut cursor = Cursor::new(&data[8..]);
    T::deserialize_reader(&mut cursor).unwrap()
}

fn setup_lottery(
    ctx: &mut TestContext,
    program_id: Pubkey,
    authority: &Keypair,
) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let (config_pda, _) = Pubkey::find_program_address(&[b"config"], &program_id);
    let init_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new(config_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: borsh::to_vec(&Instruction::Initialize {
            ticket_price_lamports: 1_000_000,
            service_charge_bps: 500,
            max_winners_cap: 32,
        })
        .unwrap(),
    };
    ctx.send_tx(vec![init_ix], &[authority]).unwrap();

    let id_le = 1u64.to_le_bytes();
    let (lottery_pda, _) =
        Pubkey::find_program_address(&[b"lottery", config_pda.as_ref(), &id_le], &program_id);
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", lottery_pda.as_ref()], &program_id);
    let (vote_tally_pda, _) =
        Pubkey::find_program_address(&[b"vote_tally", lottery_pda.as_ref()], &program_id);
    let create_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: borsh::to_vec(&Instruction::CreateLottery).unwrap(),
    };
    ctx.send_tx(vec![create_ix], &[authority]).unwrap();

    (config_pda, lottery_pda, vault_pda, vote_tally_pda)
}

#[test]
fn checked_scheduled_creation_rejects_mismatch_and_preserves_daily_uniqueness() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority]);
    let (config, _, _, _) = setup_lottery(&mut ctx, program_id, &authority);
    let before: Config = read_after_disc(&ctx.get_account(config).unwrap().data);
    let expected = before.preset_hash(&program_id, &config);
    let start = 86_400i64;
    let create = |id: u64, hash: [u8; 32]| {
        let lottery = Pubkey::find_program_address(
            &[b"lottery", config.as_ref(), &id.to_le_bytes()],
            &program_id,
        )
        .0;
        let vault = Pubkey::find_program_address(&[b"vault", lottery.as_ref()], &program_id).0;
        SdkIx {
            program_id,
            accounts: vec![
                AccountMeta::new(config, false),
                AccountMeta::new(lottery, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: borsh::to_vec(&Instruction::CreateScheduledLotteryChecked {
                buy_start_unix: start,
                expected_preset_hash: hash,
            })
            .unwrap(),
        }
    };
    let mut stale = expected;
    stale[0] ^= 1;
    let failed = create(2, stale);
    let round_address = failed.accounts[1].pubkey;
    let vault_address = failed.accounts[2].pubkey;
    common::assert_custom_error(
        ctx.send_tx(vec![failed], &[&authority]).unwrap_err(),
        Error::PresetMismatch as u32,
    );
    assert!(ctx.get_account(round_address).is_none());
    assert!(ctx.get_account(vault_address).is_none());
    let after_failure: Config = read_after_disc(&ctx.get_account(config).unwrap().data);
    assert_eq!(after_failure.lottery_count, 1);
    ctx.send_tx(vec![create(2, expected)], &[&authority])
        .unwrap();
    let round: Lottery = read_after_disc(&ctx.get_account(round_address).unwrap().data);
    assert_eq!(round.buy_start_unix, start);
    let after: Config = read_after_disc(&ctx.get_account(config).unwrap().data);
    assert_eq!(after.lottery_count, 2);
    assert_eq!(after.preset_hash(&program_id, &config), expected);
    let duplicate = create(3, expected);
    let duplicate_address = duplicate.accounts[1].pubkey;
    assert!(ctx.send_tx(vec![duplicate], &[&authority]).is_err());
    assert!(ctx.get_account(duplicate_address).is_none());
    let after_duplicate: Config = read_after_disc(&ctx.get_account(config).unwrap().data);
    assert_eq!(after_duplicate.lottery_count, 2);
}

#[test]
fn active_rules_cannot_be_changed_by_authority() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority]);

    let (config_pda, lottery_pda, _, _) = setup_lottery(&mut ctx, program_id, &authority);

    let update_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new_readonly(authority.pubkey(), true),
        ],
        data: borsh::to_vec(&Instruction::UpdateServiceCharge { new_bps: 250 }).unwrap(),
    };
    let result = ctx.send_tx(vec![update_ix], &[&authority]);
    if cfg!(feature = "allow-service-charge-update") {
        result.unwrap();
    } else {
        assert!(result.is_err());
    }

    let adjust_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new_readonly(authority.pubkey(), true),
        ],
        data: borsh::to_vec(&Instruction::BeginRevealPhase).unwrap(),
    };
    assert!(ctx.send_tx(vec![adjust_ix], &[&authority]).is_err());

    let cfg_acc = ctx.get_account(config_pda).unwrap();
    let cfg: Config = read_after_disc(&cfg_acc.data);
    assert_eq!(
        cfg.service_charge_bps,
        if cfg!(feature = "allow-service-charge-update") {
            250
        } else {
            500
        }
    );
    let account = ctx.get_account(lottery_pda).unwrap();
    let lottery: Lottery = read_after_disc(&account.data);
    assert_eq!(lottery.service_charge_bps, 500);
    assert_eq!(lottery.buy_start_unix, lottery.created_at_unix);
}

#[test]
fn buy_zero_tickets_should_fail() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let buyer = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority, &buyer]);

    let (config_pda, lottery_pda, vault_pda, _) = setup_lottery(&mut ctx, program_id, &authority);
    let (participant_pda, _) = Pubkey::find_program_address(
        &[
            b"participant",
            lottery_pda.as_ref(),
            buyer.pubkey().as_ref(),
        ],
        &program_id,
    );
    let buy_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(participant_pda, false),
            AccountMeta::new(buyer.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: borsh::to_vec(&Instruction::BuyTickets {
            proof_of_chance_hash: Some([1u8; 32]),
            number_of_tickets: 0,
        })
        .unwrap(),
    };
    let result = ctx.send_tx(vec![buy_ix], &[&buyer]);
    assert!(result.is_err());
}

#[test]
fn unauthorized_service_charge_update_should_fail() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let attacker = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority, &attacker]);

    let (config_pda, _, _, _) = setup_lottery(&mut ctx, program_id, &authority);

    let update_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new_readonly(attacker.pubkey(), true),
        ],
        data: borsh::to_vec(&Instruction::UpdateServiceCharge { new_bps: 250 }).unwrap(),
    };
    let res = ctx.send_tx(vec![update_ix], &[&attacker]);
    assert!(res.is_err());
}

#[test]
fn invalid_reveal_window_should_fail() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority]);

    let (config_pda, lottery_pda, _, _) = setup_lottery(&mut ctx, program_id, &authority);

    let begin_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new_readonly(authority.pubkey(), true),
        ],
        data: borsh::to_vec(&Instruction::BeginRevealPhase).unwrap(),
    };
    assert!(ctx.send_tx(vec![begin_ix], &[&authority]).is_err());
}

#[test]
fn settle_with_no_tickets_should_fail() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority]);

    let (config_pda, lottery_pda, vault_pda, vote_tally_pda) =
        setup_lottery(&mut ctx, program_id, &authority);

    let upload_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(vote_tally_pda, false),
        ],
        data: borsh::to_vec(&Instruction::UploadReveals { entries: vec![] }).unwrap(),
    };
    let upload_res = ctx.send_tx(vec![upload_ix], &[&authority]);
    assert!(upload_res.is_err());

    let settle_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(authority.pubkey(), false),
            AccountMeta::new(authority.pubkey(), false),
        ],
        data: borsh::to_vec(&Instruction::FinalizeWinners).unwrap(),
    };
    let res = ctx.send_tx(vec![settle_ix], &[]);
    assert!(res.is_err());
}

#[test]
fn upload_reveals_mismatch_should_fail() {
    let program_id = Pubkey::new_unique();
    let authority = Keypair::new();
    let mut ctx = TestContext::new(program_id, &[&authority]);

    let (config_pda, lottery_pda, _, vote_tally_pda) =
        setup_lottery(&mut ctx, program_id, &authority);

    let upload_ix = SdkIx {
        program_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lottery_pda, false),
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(vote_tally_pda, false),
        ],
        data: borsh::to_vec(&Instruction::UploadReveals {
            entries: vec![(Pubkey::new_unique(), b"oops".to_vec())],
        })
        .unwrap(),
    };
    let res = ctx.send_tx(vec![upload_ix], &[&authority]);
    assert!(res.is_err());
}
