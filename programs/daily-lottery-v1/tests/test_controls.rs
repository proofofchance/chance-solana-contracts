mod common;

use borsh::BorshDeserialize;
#[cfg(feature = "allow-early-upload")]
use borsh::BorshSerialize;
use common::{assert_custom_error, TestContext};
#[cfg(feature = "allow-early-upload")]
use daily_lottery::state::sizes::DEFAULT_REMEDIATION_WINDOW_SECS;
use daily_lottery::{Error, Instruction, Lottery};
use solana_instruction::{AccountMeta as M, Instruction as Ix};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::program as system;

struct Round {
    ctx: TestContext,
    authority: Keypair,
    config: Pubkey,
    lottery: Pubkey,
    vault: Pubkey,
}

impl Round {
    fn new() -> Self {
        #[cfg(any(feature = "local-test-controls", feature = "staging-test-controls"))]
        let (program, registry, authority) = (
            daily_lottery::TEST_CONTROL_PROGRAM_ID,
            daily_lottery::TEST_CONTROL_REGISTRY_ID,
            Keypair::new_from_array([7; 32]), // Isolated public fixture only.
        );
        #[cfg(not(any(feature = "local-test-controls", feature = "staging-test-controls")))]
        let (program, registry, authority) =
            (Pubkey::new_unique(), Pubkey::new_unique(), Keypair::new());
        let mut ctx = TestContext::new_with_registry(program, registry, &[&authority]);
        let config = Pubkey::find_program_address(&[b"config"], &program).0;
        let lottery = Pubkey::find_program_address(
            &[b"lottery", config.as_ref(), &1u64.to_le_bytes()],
            &program,
        )
        .0;
        let vault = Pubkey::find_program_address(&[b"vault", lottery.as_ref()], &program).0;
        let initialize = Ix {
            program_id: program,
            accounts: vec![
                M::new(authority.pubkey(), true),
                M::new(config, false),
                M::new_readonly(system::id(), false),
            ],
            data: borsh::to_vec(&Instruction::Initialize {
                ticket_price_lamports: 1_000_000,
                service_charge_bps: 500,
                max_winners_cap: 32,
            })
            .unwrap(),
        };
        ctx.send_tx(vec![initialize], &[&authority]).unwrap();
        let create = Ix {
            program_id: program,
            accounts: vec![
                M::new(config, false),
                M::new(lottery, false),
                M::new(vault, false),
                M::new(authority.pubkey(), true),
                M::new_readonly(system::id(), false),
            ],
            data: borsh::to_vec(&Instruction::CreateLottery).unwrap(),
        };
        ctx.send_tx(vec![create], &[&authority]).unwrap();
        Self {
            ctx,
            authority,
            config,
            lottery,
            vault,
        }
    }

    fn state(&self) -> Lottery {
        Lottery::deserialize(&mut &self.ctx.get_account(self.lottery).unwrap().data[8..]).unwrap()
    }

    #[cfg(feature = "allow-early-upload")]
    fn set_state(&mut self, lottery: &Lottery) {
        let mut account = self.ctx.get_account(self.lottery).unwrap();
        lottery.serialize(&mut &mut account.data[8..]).unwrap();
        self.ctx.set_account(self.lottery, account);
    }

    fn at(&mut self, timestamp: i64) {
        let mut clock = self.ctx.get_clock();
        clock.unix_timestamp = timestamp;
        self.ctx.set_clock(&clock);
        self.ctx.svm.expire_blockhash();
    }

    fn begin(&self) -> Ix {
        let registry_config = Pubkey::find_program_address(
            &[b"registry", self.ctx.payer.pubkey().as_ref()],
            &self.ctx.registry,
        )
        .0;
        let release = Pubkey::find_program_address(
            &[
                b"release",
                registry_config.as_ref(),
                self.ctx.daily_program.as_ref(),
            ],
            &self.ctx.registry,
        )
        .0;
        Ix {
            program_id: self.ctx.daily_program,
            accounts: vec![
                M::new_readonly(self.config, false),
                M::new(self.lottery, false),
                M::new_readonly(self.authority.pubkey(), true),
                M::new_readonly(self.ctx.registry, false),
                M::new_readonly(registry_config, false),
                M::new_readonly(release, false),
            ],
            data: vec![8],
        }
    }

    #[cfg(feature = "allow-early-upload")]
    fn buy(&mut self, buyer: &Keypair) -> solana_instruction::Instruction {
        self.ctx
            .svm
            .airdrop(&buyer.pubkey(), 1_000_000_000)
            .unwrap();
        let participant = Pubkey::find_program_address(
            &[
                b"participant",
                self.lottery.as_ref(),
                buyer.pubkey().as_ref(),
            ],
            &self.ctx.daily_program,
        )
        .0;
        Ix {
            program_id: self.ctx.daily_program,
            accounts: vec![
                M::new_readonly(self.config, false),
                M::new(self.lottery, false),
                M::new(self.vault, false),
                M::new(participant, false),
                M::new(buyer.pubkey(), true),
                M::new_readonly(system::id(), false),
            ],
            data: borsh::to_vec(&Instruction::BuyTickets {
                proof_of_chance_hash: Some([42; 32]),
                number_of_tickets: 1,
            })
            .unwrap(),
        }
    }
}

#[test]
fn executing_artifact_reports_fixed_capabilities_without_mutation() {
    let mut round = Round::new();
    let before = round.ctx.get_account(round.lottery).unwrap();
    assert_eq!(
        borsh::to_vec(&Instruction::GetBuildCapabilities).unwrap(),
        vec![21]
    );
    let result = round
        .ctx
        .send_tx(
            vec![Ix {
                program_id: round.ctx.daily_program,
                accounts: vec![],
                data: vec![21],
            }],
            &[],
        )
        .unwrap();
    assert_eq!(result.return_data.program_id, round.ctx.daily_program);
    let data = result.return_data.data;
    assert_eq!(data.len(), 110);
    assert_eq!(&data[..8], b"CHCTL001");
    assert_eq!(&data[8..12], &[1, 0, 1, 0]);
    assert_eq!(
        data[12],
        if cfg!(feature = "local-test-controls") {
            1
        } else if cfg!(feature = "staging-test-controls") {
            2
        } else {
            0
        }
    );
    assert_eq!(
        data[13],
        u8::from(cfg!(feature = "allow-early-upload"))
            | (u8::from(cfg!(feature = "allow-service-charge-update")) << 1)
    );
    assert_eq!(&data[14..46], round.ctx.daily_program.as_ref());
    assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
}

#[cfg(not(feature = "allow-early-upload"))]
#[test]
fn production_and_unenabled_scopes_reject_valid_provider_without_writing_accounts() {
    let mut round = Round::new();
    round.at(round.state().buy_start_unix + 10);
    let before = round.ctx.get_account(round.lottery).unwrap();
    let vault_before = round.ctx.get_account(round.vault).unwrap();
    for data in [vec![8], [vec![6], vec![0; 8]].concat()] {
        let mut instruction = round.begin();
        instruction.data = data;
        assert_custom_error(
            round
                .ctx
                .send_tx(vec![instruction], &[&round.authority])
                .unwrap_err(),
            Error::InvalidPhaseTransition as u32,
        );
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
        assert_eq!(round.ctx.get_account(round.vault).unwrap(), vault_before);
    }
}

#[test]
fn duration_editor_and_trailing_bytes_never_enable_early_upload() {
    let mut round = Round::new();
    let before = round.ctx.get_account(round.lottery).unwrap();
    for (data, error) in [
        (
            [vec![6], vec![1; 8]].concat(),
            Error::InvalidPhaseTransition,
        ),
        (vec![8, 0], Error::InvalidInstruction),
    ] {
        let mut instruction = round.begin();
        instruction.data = data;
        assert_custom_error(
            round
                .ctx
                .send_tx(vec![instruction], &[&round.authority])
                .unwrap_err(),
            error as u32,
        );
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
    }
}

#[cfg(feature = "allow-early-upload")]
#[test]
fn one_shot_cutoff_preserves_duration_identity_and_funds_for_zero_one_and_two_buyers() {
    for count in 0..=2 {
        let mut round = Round::new();
        for _ in 0..count {
            let buyer = Keypair::new();
            let ix = round.buy(&buyer);
            round.ctx.send_tx(vec![ix], &[&buyer]).unwrap();
        }
        let original = round.state();
        let now = original.buy_start_unix + 10;
        round.at(now);
        let vault = round.ctx.get_account(round.vault).unwrap();
        let config = round.ctx.get_account(round.config).unwrap();
        let ix = round.begin();
        let result = round
            .ctx
            .send_tx(vec![ix.clone()], &[&round.authority])
            .unwrap();
        let accelerated = round.state();
        assert_eq!(accelerated.buy_deadline_unix, now);
        assert_eq!(accelerated.upload_start_unix, now);
        assert_eq!(
            accelerated.upload_deadline_unix,
            now + original.upload_deadline_unix - original.upload_start_unix
        );
        assert_eq!(
            accelerated.settlement_deadline_unix,
            accelerated.upload_deadline_unix + 2 * DEFAULT_REMEDIATION_WINDOW_SECS
        );
        assert!(!accelerated.is_in_buy_window(now));
        assert!(accelerated.is_in_upload_window(now));
        assert!(!accelerated.is_in_upload_window(accelerated.upload_deadline_unix));
        let mut normalized = accelerated.clone();
        normalized.buy_deadline_unix = original.buy_deadline_unix;
        normalized.upload_start_unix = original.upload_start_unix;
        normalized.upload_deadline_unix = original.upload_deadline_unix;
        normalized.settlement_deadline_unix = original.settlement_deadline_unix;
        assert_eq!(accelerated.reserved[0], original.reserved[0] | 1);
        normalized.reserved[0] = original.reserved[0];
        assert_eq!(
            borsh::to_vec(&normalized).unwrap(),
            borsh::to_vec(&original).unwrap()
        );
        assert_eq!(round.ctx.get_account(round.vault).unwrap(), vault);
        assert_eq!(round.ctx.get_account(round.config).unwrap(), config);
        let event = result
            .logs
            .iter()
            .find(|log| log.contains("TestUploadPhaseBegan"))
            .expect("complete timing event");
        for field in [
            "begin-upload-v1",
            "old_buy_deadline",
            "new_settlement_deadline",
            "new_upload_start",
        ] {
            assert!(event.contains(field));
        }
        round.at(now + 1);
        let before = round.ctx.get_account(round.lottery).unwrap();
        assert_custom_error(
            round
                .ctx
                .send_tx(vec![ix], &[&round.authority])
                .unwrap_err(),
            Error::InvalidPhaseTransition as u32,
        );
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
        let buyer = Keypair::new();
        let buy = round.buy(&buyer);
        assert_custom_error(
            round.ctx.send_tx(vec![buy], &[&buyer]).unwrap_err(),
            Error::OutsideTimeWindow as u32,
        );
        // Even a backwards clock cannot reuse the control or reopen buying.
        round.at(original.buy_start_unix + 1);
        let instruction = round.begin();
        assert_custom_error(
            round
                .ctx
                .send_tx(vec![instruction], &[&round.authority])
                .unwrap_err(),
            Error::InvalidPhaseTransition as u32,
        );
        let buyer = Keypair::new();
        let buy = round.buy(&buyer);
        assert_custom_error(
            round.ctx.send_tx(vec![buy], &[&buyer]).unwrap_err(),
            Error::OutsideTimeWindow as u32,
        );
    }
}

#[cfg(feature = "allow-early-upload")]
#[test]
fn origin_authority_and_invalid_lifecycle_states_fail_atomically() {
    let mut round = Round::new();
    let original = round.state();
    round.at(original.buy_start_unix + 10);
    let attacker = Keypair::new();
    round
        .ctx
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();
    let before = round.ctx.get_account(round.lottery).unwrap();
    let mut attack = round.begin();
    attack.accounts[2] = M::new_readonly(attacker.pubkey(), true);
    assert_custom_error(
        round.ctx.send_tx(vec![attack], &[&attacker]).unwrap_err(),
        Error::Unauthorized as u32,
    );
    assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
    for index in [0, 1, 3, 4, 5] {
        let mut attack = round.begin();
        attack.accounts[index] = M::new_readonly(system::id(), false);
        assert!(round
            .ctx
            .send_tx(vec![attack], &[&round.authority])
            .is_err());
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
    }
    for case in 0..9 {
        let mut changed = original.clone();
        match case {
            0 => changed.attested_count = 1,
            1 => changed.provider_uploaded_count = 1,
            2 => changed.settled = true,
            3 => changed.remediation_start_unix = 1,
            4 => changed.winners_count = 1,
            5 => changed.status = 255,
            6 => changed.settlement_start_unix = 1,
            7 => changed.uploads_complete = true,
            _ => changed.settlement_deadline_unix -= 1,
        }
        round.set_state(&changed);
        round.ctx.svm.expire_blockhash();
        let before = round.ctx.get_account(round.lottery).unwrap();
        let ix = round.begin();
        assert!(round.ctx.send_tx(vec![ix], &[&round.authority]).is_err());
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
    }
    round.set_state(&original);
    for now in [
        original.buy_start_unix - 1,
        original.buy_deadline_unix,
        original.upload_deadline_unix,
    ] {
        round.at(now);
        let before = round.ctx.get_account(round.lottery).unwrap();
        let ix = round.begin();
        assert_custom_error(
            round
                .ctx
                .send_tx(vec![ix], &[&round.authority])
                .unwrap_err(),
            Error::InvalidPhaseTransition as u32,
        );
        assert_eq!(round.ctx.get_account(round.lottery).unwrap(), before);
    }
}
