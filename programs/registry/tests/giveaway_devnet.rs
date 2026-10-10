#![allow(clippy::result_large_err)]
use chance_registry_wire as wire;
use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_clock::Clock;
use solana_instruction::{AccountMeta as M, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::program as system;
use solana_transaction::Transaction;

fn anchor<T: borsh::BorshSerialize>(name: &str, args: &T) -> Vec<u8> {
    [
        Sha256::digest(format!("global:{name}").as_bytes())[..8].to_vec(),
        wire::to_vec(args).unwrap(),
    ]
    .concat()
}

fn stored(data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports: 10_000_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

#[test]
#[ignore = "requires the separately reproduced devnet-v1 ELF"]
fn fixed_bootstrap_permissionless_creators_and_retirement_preserve_existing_instances() {
    let program = solana_program::pubkey!("FQxfsBE7fXuBbcxERsgRwi1AHsbSMdU7nWwhGz8ZRLUY");
    let authority = solana_program::pubkey!("HMroJo6qBsFiodxFJeqU8VDBLif5P5kDFWmEFpibaKCQ");
    let registry = Pubkey::new_unique();
    let governance = Pubkey::new_unique();
    let registry_config =
        Pubkey::find_program_address(&[b"registry", governance.as_ref()], &registry).0;
    let release_address = Pubkey::find_program_address(
        &[b"release", registry_config.as_ref(), program.as_ref()],
        &registry,
    )
    .0;
    let config = Pubkey::find_program_address(&[b"config"], &program).0;
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(
        program,
        std::env::var("CHANCE_GIVEAWAY_DEVNET_ELF").expect("qualified ELF path required"),
    )
    .unwrap();
    svm.add_program_from_file(
        registry,
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/deploy/chance_registry.so"
        ),
    )
    .unwrap();
    let registry_state = wire::Config {
        tag: wire::CONFIG_TAG,
        schema: wire::SCHEMA,
        authority: governance.to_bytes(),
        guardian: Pubkey::new_unique().to_bytes(),
        activation_delay: 1,
        release_count: 1,
        record_count: 1,
        instance_count: 0,
    };
    let mut release = wire::Release {
        tag: wire::RELEASE_TAG,
        registry: registry_config.to_bytes(),
        program: program.to_bytes(),
        domain: wire::GIVEAWAY,
        series: [1; 32],
        source_hash: [2; 32],
        executable_hash: [3; 32],
        sequence: 1,
        registered_slot: 1,
        eligible_at: 0,
        status: wire::ACTIVE,
        successor: [0; 32],
    };
    // Isolated CPI fixture: registration/immutable activation is covered by the
    // registry lifecycle suite. Here the actual release ELF and registry execute.
    svm.set_account(
        registry_config,
        stored(wire::to_vec(&registry_state).unwrap(), registry),
    )
    .unwrap();
    svm.set_account(
        release_address,
        stored(wire::to_vec(&release).unwrap(), registry),
    )
    .unwrap();
    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let initialize = |who| Instruction {
        program_id: program,
        accounts: vec![
            M::new(config, false),
            M::new(who, true),
            M::new_readonly(system::id(), false),
        ],
        data: anchor("initialize", &(500u16, 3_600u32, 3_600u32)),
    };
    let failed = Transaction::new_signed_with_payer(
        &[initialize(attacker.pubkey())],
        Some(&attacker.pubkey()),
        &[&attacker],
        svm.latest_blockhash(),
    );
    assert!(svm.send_transaction(failed).is_err());
    assert!(svm.get_account(&config).is_none());

    // No live bootstrap key enters CI. Disable signature cryptography solely
    // for this fixed public bootstrap signer; runtime signer checks still apply.
    // Creator tests below restore cryptographic signature verification.
    svm = svm.with_sigverify(false);
    svm.airdrop(&authority, 1_000_000_000).unwrap();
    let bind = Instruction {
        program_id: program,
        accounts: vec![
            M::new(config, false),
            M::new_readonly(authority, true),
            M::new_readonly(registry, false),
            M::new_readonly(registry_config, false),
            M::new_readonly(release_address, false),
        ],
        data: anchor("bind_registry", &()),
    };
    let mut setup = Transaction::new_with_payer(&[initialize(authority), bind], Some(&authority));
    setup.message.recent_blockhash = svm.latest_blockhash();
    svm.send_transaction(setup).unwrap();
    svm = svm.with_sigverify(true);
    let start = svm.get_sysvar::<Clock>().unix_timestamp;
    let creators = [Keypair::new(), Keypair::new()];
    let mut retained = Vec::new();
    for (index, creator) in creators.iter().enumerate() {
        svm.airdrop(&creator.pubkey(), 1_000_000_000).unwrap();
        assert_ne!(creator.pubkey(), authority);
        let id = index as u64 + 1;
        let nonce = 42u64;
        let instance = Pubkey::find_program_address(
            &[b"giveaway", config.as_ref(), &id.to_le_bytes()],
            &program,
        )
        .0;
        let vault = Pubkey::find_program_address(&[b"vault", instance.as_ref()], &program).0;
        let business_hash = Sha256::digest(
            [
                b"chance-giveaway-key-v1".as_slice(),
                creator.pubkey().as_ref(),
                &nonce.to_le_bytes(),
            ]
            .concat(),
        );
        let business = Pubkey::find_program_address(
            &[
                b"business",
                registry_config.as_ref(),
                &[wire::GIVEAWAY],
                &business_hash,
            ],
            &registry,
        )
        .0;
        let entry = Pubkey::find_program_address(
            &[b"instance", registry_config.as_ref(), &id.to_le_bytes()],
            &registry,
        )
        .0;
        let expected: [u8; 32] = Sha256::digest(
            [
                b"CHANCE_GIVEAWAY_CREATION_V1".as_slice(),
                program.as_ref(),
                config.as_ref(),
                registry.as_ref(),
                registry_config.as_ref(),
                authority.as_ref(),
                &500u16.to_le_bytes(),
                creator.pubkey().as_ref(),
                &id.to_le_bytes(),
                &1_000_000u64.to_le_bytes(),
                &1u32.to_le_bytes(),
                &start.to_le_bytes(),
                &(start + 3_600).to_le_bytes(),
                &3_600u32.to_le_bytes(),
                &nonce.to_le_bytes(),
            ]
            .concat(),
        )
        .into();
        let instruction = Instruction {
            program_id: program,
            accounts: vec![
                M::new(config, false),
                M::new(instance, false),
                M::new(vault, false),
                M::new(creator.pubkey(), true),
                M::new_readonly(system::id(), false),
                M::new_readonly(registry, false),
                M::new(registry_config, false),
                M::new_readonly(release_address, false),
                M::new_readonly(
                    Pubkey::find_program_address(
                        &[b"chance-release", registry_config.as_ref()],
                        &program,
                    )
                    .0,
                    false,
                ),
                M::new(entry, false),
                M::new(business, false),
            ],
            data: anchor(
                "create_giveaway_checked",
                &(
                    id,
                    1_000_000u64,
                    1u32,
                    start,
                    start + 3_600,
                    3_600u32,
                    nonce,
                    expected,
                ),
            ),
        };
        svm.expire_blockhash();
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&creator.pubkey()),
            &[creator],
            svm.latest_blockhash(),
        );
        let result = svm.send_transaction(transaction).unwrap();
        assert!(result.logs.iter().any(|log| log.contains("GIVEAWAY_EVENT")));
        let account = svm.get_account(&instance).unwrap();
        assert_eq!(&account.data[265..297], authority.as_ref());
        assert!(svm.get_account(&vault).unwrap().lamports >= 1_000_000);
        assert_eq!(svm.get_account(&vault).unwrap().owner, program);
        let record: wire::Instance =
            wire::from_slice(&svm.get_account(&entry).unwrap().data).unwrap();
        assert_eq!(record.creator, creator.pubkey().to_bytes());
        retained.push((instance, account.data));
    }
    release.status = wire::DISCONTINUED;
    svm.set_account(
        release_address,
        stored(wire::to_vec(&release).unwrap(), registry),
    )
    .unwrap();
    for (instance, data) in retained {
        assert_eq!(svm.get_account(&instance).unwrap().data, data);
    }
    let state: wire::Config =
        wire::from_slice(&svm.get_account(&registry_config).unwrap().data).unwrap();
    assert_eq!(state.instance_count, 2);
}
