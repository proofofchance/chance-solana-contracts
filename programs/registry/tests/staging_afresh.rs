#![cfg(feature = "staging-afresh-v1")]
#![allow(clippy::result_large_err)]
//! Isolated SBF qualification of fresh address and bootstrap signer bindings.
use chance_registry_wire as wire;
use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_instruction::{AccountMeta as M, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::program as system;
use solana_transaction::Transaction;
use std::str::FromStr;

fn identities() -> std::collections::BTreeMap<&'static str, &'static str> {
    include_str!("../../../releases/staging-afresh-v1.inputs")
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect()
}

fn instruction(program: Pubkey, config: Pubkey, authority: Pubkey, domain: &str) -> Instruction {
    let (accounts, data) = match domain {
        "registry" => (
            vec![
                M::new(config, false),
                M::new(authority, true),
                M::new_readonly(system::id(), false),
            ],
            wire::to_vec(&wire::Instruction::Initialize {
                guardian: [9; 32],
                activation_delay: 1,
            })
            .unwrap(),
        ),
        "daily" => (
            vec![
                M::new(authority, true),
                M::new(config, false),
                M::new_readonly(system::id(), false),
            ],
            [
                vec![0],
                1_000_000u64.to_le_bytes().to_vec(),
                500u16.to_le_bytes().to_vec(),
                256u32.to_le_bytes().to_vec(),
            ]
            .concat(),
        ),
        "giveaway" => (
            vec![
                M::new(config, false),
                M::new(authority, true),
                M::new_readonly(system::id(), false),
            ],
            [
                Sha256::digest(b"global:initialize")[..8].to_vec(),
                wire::to_vec(&(500u16, 3_600u32, 3_600u32)).unwrap(),
            ]
            .concat(),
        ),
        _ => unreachable!(),
    };
    Instruction {
        program_id: program,
        accounts,
        data,
    }
}

#[test]
fn each_fresh_v1_elf_rejects_foreign_bootstrap_and_accepts_only_compiled_identity() {
    let inputs = identities();
    let authority = Pubkey::from_str(inputs["initialization_authority"]).unwrap();
    for (domain, key, library) in [
        ("registry", "registry_program", "chance_registry"),
        ("daily", "daily_lottery_program", "daily_lottery"),
        ("giveaway", "giveaways_program", "giveaways"),
    ] {
        let program = Pubkey::from_str(inputs[key]).unwrap();
        let config = if domain == "registry" {
            Pubkey::find_program_address(&[b"registry", authority.as_ref()], &program).0
        } else {
            Pubkey::find_program_address(&[b"config"], &program).0
        };
        let mut svm = LiteSVM::new();
        let elf = std::path::PathBuf::from(
            std::env::var("CHANCE_AFRESH_ELF_DIR")
                .expect("explicit reproduced fresh ELF directory required"),
        )
        .join(format!("{library}.so"));
        svm.add_program_from_file(program, &elf).unwrap();
        let attacker = Keypair::new();
        svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
        let attacker_config = if domain == "registry" {
            Pubkey::find_program_address(&[b"registry", attacker.pubkey().as_ref()], &program).0
        } else {
            config
        };
        let attack = Transaction::new_signed_with_payer(
            &[instruction(
                program,
                attacker_config,
                attacker.pubkey(),
                domain,
            )],
            Some(&attacker.pubkey()),
            &[&attacker],
            svm.latest_blockhash(),
        );
        assert!(
            svm.send_transaction(attack).is_err(),
            "{domain} foreign initializer accepted"
        );
        assert!(
            svm.get_account(&attacker_config).is_none(),
            "{domain} foreign initializer wrote state"
        );
        // CI retains only the public feature input. Disable cryptographic
        // verification solely to exercise its fixed signer flag; this is an
        // isolated LiteSVM instance, never a deployer or runtime escape hatch.
        svm = svm.with_sigverify(false);
        svm.airdrop(&authority, 1_000_000_000).unwrap();
        let mut initialize = Transaction::new_with_payer(
            &[instruction(program, config, authority, domain)],
            Some(&authority),
        );
        initialize.message.recent_blockhash = svm.latest_blockhash();
        svm.send_transaction(initialize).unwrap();
        assert_eq!(svm.get_account(&config).unwrap().owner, program);
        let wrong_program = Pubkey::new_unique();
        svm.add_program_from_file(wrong_program, &elf).unwrap();
        let wrong_config = if domain == "registry" {
            Pubkey::find_program_address(&[b"registry", authority.as_ref()], &wrong_program).0
        } else {
            Pubkey::find_program_address(&[b"config"], &wrong_program).0
        };
        let mut wrong = Transaction::new_with_payer(
            &[instruction(wrong_program, wrong_config, authority, domain)],
            Some(&authority),
        );
        wrong.message.recent_blockhash = svm.latest_blockhash();
        assert!(
            svm.send_transaction(wrong).is_err(),
            "{domain} wrong runtime identity accepted"
        );
        assert!(svm.get_account(&wrong_config).is_none());
    }
}
