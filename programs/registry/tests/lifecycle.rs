#![allow(clippy::result_large_err)]
use chance_registry_wire as wire;
use litesvm::LiteSVM;
use solana_clock::Clock;
use solana_instruction::{AccountMeta as M, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::program as system;
use solana_transaction::Transaction;

struct Fixture {
    svm: LiteSVM,
    program: Pubkey,
    admin: Keypair,
    guardian: Keypair,
    cfg: Pubkey,
    history: Vec<serde_json::Value>,
}
impl Fixture {
    fn new() -> Self {
        let program = Pubkey::new_unique();
        let admin = Keypair::new();
        let guardian = Keypair::new();
        let cfg = Pubkey::find_program_address(&[b"registry", admin.pubkey().as_ref()], &program).0;
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(
            program,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/deploy/chance_registry.so"
            ),
        )
        .unwrap();
        svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
        svm.airdrop(&guardian.pubkey(), 10_000_000_000).unwrap();
        let mut f = Self {
            svm,
            program,
            admin,
            guardian,
            cfg,
            history: vec![],
        };
        f.send(
            wire::Instruction::Initialize {
                guardian: f.guardian.pubkey().to_bytes(),
                activation_delay: 60,
            },
            vec![
                M::new(cfg, false),
                M::new(f.admin.pubkey(), true),
                M::new_readonly(system::id(), false),
            ],
            false,
        )
        .unwrap();
        f
    }
    fn send(
        &mut self,
        data: wire::Instruction,
        accounts: Vec<M>,
        guardian: bool,
    ) -> litesvm::types::TransactionResult {
        self.svm.expire_blockhash();
        let payer = if guardian {
            &self.guardian
        } else {
            &self.admin
        };
        let tx = Transaction::new_signed_with_payer(
            &[Instruction {
                program_id: self.program,
                accounts,
                data: wire::to_vec(&data).unwrap(),
            }],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.submit(tx)
    }
    fn submit(&mut self, tx: Transaction) -> litesvm::types::TransactionResult {
        let keys = tx.message.account_keys.clone();
        let pre: Vec<u64> = keys
            .iter()
            .map(|key| {
                self.svm
                    .get_account(key)
                    .map_or(0, |account| account.lamports)
            })
            .collect();
        let result = self.svm.send_transaction(tx.clone());
        if let Ok(metadata) = &result {
            let post: Vec<u64> = keys
                .iter()
                .map(|key| {
                    self.svm
                        .get_account(key)
                        .map_or(0, |account| account.lamports)
                })
                .collect();
            let instructions: Vec<_> = tx.message.instructions.iter().map(|ix| {
                let hex: String = ix.data.iter().map(|byte| format!("{byte:02x}")).collect();
                serde_json::json!({"programIdIndex":ix.program_id_index, "accounts":ix.accounts, "dataHex":hex})
            }).collect();
            let fee = pre.iter().map(|n| *n as u128).sum::<u128>()
                - post.iter().map(|n| *n as u128).sum::<u128>();
            self.history.push(serde_json::json!({"slot":self.svm.get_sysvar::<Clock>().slot,
                "transaction":{"signatures":tx.signatures.iter().map(|signature| signature.to_string()).collect::<Vec<_>>(),
                    "message":{"accountKeys":keys.iter().map(|key| key.to_string()).collect::<Vec<_>>(),
                    "header":{"numRequiredSignatures":tx.message.header.num_required_signatures}, "instructions":instructions}},
                "meta":{"err":null,"logMessages":metadata.logs,"preBalances":pre,"postBalances":post,"fee":fee as u64}}));
        }
        result
    }
    fn state(&self) -> wire::Config {
        wire::from_slice(&self.svm.get_account(&self.cfg).unwrap().data).unwrap()
    }
    fn register(&mut self, target: Pubkey) -> Pubkey {
        self.register_domain(target, wire::DAILY)
    }
    fn register_domain(&mut self, target: Pubkey, domain: u8) -> Pubkey {
        let rel = Pubkey::find_program_address(
            &[b"release", self.cfg.as_ref(), target.as_ref()],
            &self.program,
        )
        .0;
        self.send(
            wire::Instruction::Register {
                domain,
                series: [1; 32],
                source_hash: [2; 32],
                executable_hash: self.deployed_hash(target),
            },
            vec![
                M::new(self.cfg, false),
                M::new(rel, false),
                M::new(self.admin.pubkey(), true),
                M::new_readonly(system::id(), false),
                M::new_readonly(target, false),
            ],
            false,
        )
        .unwrap();
        rel
    }
    fn lifecycle(
        &mut self,
        rel: Pubkey,
        instruction: wire::Instruction,
        extra: Vec<M>,
        guardian: bool,
    ) -> litesvm::types::TransactionResult {
        let record = Pubkey::find_program_address(
            &[
                b"record",
                self.cfg.as_ref(),
                &(self.state().record_count + 1).to_le_bytes(),
            ],
            &self.program,
        )
        .0;
        let actor = if guardian {
            self.guardian.pubkey()
        } else {
            self.admin.pubkey()
        };
        let mut accounts = vec![
            M::new(self.cfg, false),
            M::new(rel, false),
            M::new(actor, true),
            M::new_readonly(system::id(), false),
            M::new(record, false),
        ];
        accounts.extend(extra);
        self.send(instruction, accounts, guardian)
    }
    fn activation_accounts(&self, target: Pubkey) -> Vec<M> {
        let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
        vec![
            M::new_readonly(target, false),
            M::new_readonly(
                Pubkey::find_program_address(&[target.as_ref()], &loader).0,
                false,
            ),
            M::new_readonly(self.program, false),
            M::new_readonly(
                Pubkey::find_program_address(&[self.program.as_ref()], &loader).0,
                false,
            ),
        ]
    }
    fn advance(&mut self) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += 61;
        self.svm.set_sysvar(&clock);
    }
}

#[test]
fn delayed_activation_terminal_discontinuation_and_append_only_successor_history() {
    let mut f = Fixture::new();
    let rel = f.register(f.program);
    let args = f.activation_accounts(f.program);
    assert!(f
        .lifecycle(
            rel,
            wire::Instruction::Activate {
                reason: "checked release".into()
            },
            args.clone(),
            false
        )
        .is_err());
    assert_eq!(f.state().record_count, 0);
    f.advance();
    // Guardian may stop creation but cannot activate releases.
    assert!(f
        .lifecycle(
            rel,
            wire::Instruction::Activate {
                reason: "guardian".into()
            },
            args.clone(),
            true
        )
        .is_err());
    f.lifecycle(
        rel,
        wire::Instruction::Activate {
            reason: "checked release".into(),
        },
        args.clone(),
        false,
    )
    .unwrap();
    f.lifecycle(
        rel,
        wire::Instruction::Pause {
            reason: "review".into(),
        },
        vec![],
        true,
    )
    .unwrap();
    assert!(f
        .lifecycle(
            rel,
            wire::Instruction::Activate {
                reason: "resume".into()
            },
            args.clone(),
            false
        )
        .is_err());
    f.advance();
    f.lifecycle(
        rel,
        wire::Instruction::Activate {
            reason: "resume".into(),
        },
        args.clone(),
        false,
    )
    .unwrap();
    f.lifecycle(
        rel,
        wire::Instruction::Discontinue {
            reason: "superseded".into(),
        },
        vec![],
        true,
    )
    .unwrap();
    assert!(f
        .lifecycle(
            rel,
            wire::Instruction::Activate {
                reason: "revive".into()
            },
            args,
            false
        )
        .is_err());
    let next_program = Pubkey::new_unique();
    f.svm
        .add_program_from_file(
            next_program,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/deploy/chance_registry.so"
            ),
        )
        .unwrap();
    let next = f.register(next_program);
    f.lifecycle(
        rel,
        wire::Instruction::SetSuccessor {
            reason: "reviewed replacement".into(),
        },
        vec![M::new_readonly(next, false)],
        false,
    )
    .unwrap();
    f.lifecycle(
        next,
        wire::Instruction::Discontinue {
            reason: "also retired".into(),
        },
        vec![],
        false,
    )
    .unwrap();
    assert!(f
        .lifecycle(
            next,
            wire::Instruction::SetSuccessor {
                reason: "cycle".into()
            },
            vec![M::new_readonly(rel, false)],
            false
        )
        .is_err());
    assert_eq!(f.state().record_count, 6);
    for seq in 1u64..=6 {
        let key = Pubkey::find_program_address(
            &[b"record", f.cfg.as_ref(), &seq.to_le_bytes()],
            &f.program,
        )
        .0;
        let record: wire::Record =
            wire::from_slice(&f.svm.get_account(&key).unwrap().data).unwrap();
        assert_eq!(record.sequence, seq);
        assert!(!record.reason.is_empty());
    }
}

#[test]
fn mutable_or_spoofed_programdata_cannot_activate() {
    let mut f = Fixture::new();
    let rel = f.register(f.program);
    f.advance();
    let args = f.activation_accounts(f.program);
    let data_key = args[1].pubkey;
    let mut data = f.svm.get_account(&data_key).unwrap();
    data.data[12] = 1;
    data.data[13..45].copy_from_slice(f.admin.pubkey().as_ref());
    f.svm.set_account(data_key, data).unwrap();
    assert!(f
        .lifecycle(
            rel,
            wire::Instruction::Activate {
                reason: "mutable".into()
            },
            args,
            false
        )
        .is_err());
    assert_eq!(f.state().record_count, 0);
    let rel_state: wire::Release =
        wire::from_slice(&f.svm.get_account(&rel).unwrap().data).unwrap();
    assert_eq!(rel_state.status, wire::REGISTERED);
}

impl Fixture {
    fn deployed_hash(&self, target: Pubkey) -> [u8; 32] {
        let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
        let key = Pubkey::find_program_address(&[target.as_ref()], &loader).0;
        let bytes = self.svm.get_account(&key).unwrap().data;
        let code = &bytes[45..];
        let end = code.iter().rposition(|b| *b != 0).unwrap() + 1;
        solana_sha256_hasher::hash(&code[..end]).to_bytes()
    }
    fn external(&mut self, ix: Instruction) -> litesvm::types::TransactionResult {
        self.svm.expire_blockhash();
        let budget = Instruction {
            program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
            accounts: vec![],
            data: [vec![2], 1_400_000u32.to_le_bytes().to_vec()].concat(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[budget, ix],
            Some(&self.admin.pubkey()),
            &[&self.admin],
            self.svm.latest_blockhash(),
        );
        self.submit(tx)
    }
    fn activate_large(&mut self, rel: Pubkey, target: Pubkey) {
        let record = Pubkey::find_program_address(
            &[
                b"record",
                self.cfg.as_ref(),
                &(self.state().record_count + 1).to_le_bytes(),
            ],
            &self.program,
        )
        .0;
        let mut accounts = vec![
            M::new(self.cfg, false),
            M::new(rel, false),
            M::new(self.admin.pubkey(), true),
            M::new_readonly(system::id(), false),
            M::new(record, false),
        ];
        accounts.extend(self.activation_accounts(target));
        self.external(Instruction {
            program_id: self.program,
            accounts,
            data: wire::to_vec(&wire::Instruction::Activate {
                reason: "runtime test immutable program".into(),
            })
            .unwrap(),
        })
        .unwrap();
    }
    fn daily_setup(&mut self, target: Pubkey, rel: Pubkey) -> Pubkey {
        let cfg = Pubkey::find_program_address(&[b"config"], &target).0;
        self.external(Instruction {
            program_id: target,
            accounts: vec![
                M::new(self.admin.pubkey(), true),
                M::new(cfg, false),
                M::new_readonly(system::id(), false),
            ],
            data: [
                vec![0],
                wire::to_vec(&(1_000_000u64, 500u16, 32u32)).unwrap(),
            ]
            .concat(),
        })
        .unwrap();
        self.external(Instruction {
            program_id: target,
            accounts: vec![
                M::new(cfg, false),
                M::new_readonly(self.admin.pubkey(), true),
                M::new_readonly(self.program, false),
                M::new_readonly(self.cfg, false),
                M::new_readonly(rel, false),
            ],
            data: vec![18],
        })
        .unwrap();
        cfg
    }
    fn daily_create(
        &self,
        target: Pubkey,
        config: Pubkey,
        rel: Pubkey,
        id: u64,
        start: i64,
    ) -> Instruction {
        let lottery = Pubkey::find_program_address(
            &[b"lottery", config.as_ref(), &id.to_le_bytes()],
            &target,
        )
        .0;
        let vault = Pubkey::find_program_address(&[b"vault", lottery.as_ref()], &target).0;
        let seq = (self.state().instance_count + 1).to_le_bytes();
        let day = (start as u64 / 86_400).to_le_bytes();
        let key = solana_sha256_hasher::hashv(&[b"chance-daily-key-v1", &[1; 32], &day]).to_bytes();
        Instruction {
            program_id: target,
            accounts: vec![
                M::new(config, false),
                M::new(lottery, false),
                M::new(vault, false),
                M::new(self.admin.pubkey(), true),
                M::new_readonly(system::id(), false),
                M::new_readonly(self.program, false),
                M::new(self.cfg, false),
                M::new_readonly(rel, false),
                M::new_readonly(
                    Pubkey::find_program_address(&[b"chance-release", self.cfg.as_ref()], &target)
                        .0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(
                        &[b"instance", self.cfg.as_ref(), &seq],
                        &self.program,
                    )
                    .0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(
                        &[b"business", self.cfg.as_ref(), &[wire::DAILY], &key],
                        &self.program,
                    )
                    .0,
                    false,
                ),
            ],
            data: [vec![19], start.to_le_bytes().to_vec()].concat(),
        }
    }
}

#[test]
fn actual_daily_programs_share_uniqueness_and_survive_discontinuation() {
    let mut f = Fixture::new();
    let a = Pubkey::new_unique();
    let b = Pubkey::new_unique();
    for program in [a, b] {
        f.svm
            .add_program_from_file(
                program,
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../target/deploy/daily_lottery.so"
                ),
            )
            .unwrap();
    }
    let ra = f.register(a);
    let rb = f.register(b);
    let ca = f.daily_setup(a, ra);
    let cb = f.daily_setup(b, rb);
    f.advance();
    f.activate_large(ra, a);
    f.activate_large(rb, b);
    let now = f.svm.get_sysvar::<Clock>().unix_timestamp;
    let create = f.daily_create(a, ca, ra, 1, now);
    let mut bypass = create.clone();
    bypass.accounts.truncate(5);
    assert!(f.external(bypass).is_err());
    assert_eq!(f.state().instance_count, 0);
    // Dust at a predictable PDA cannot deny future canonical creation.
    let donation = solana_system_interface::instruction::transfer(
        &f.admin.pubkey(),
        &create.accounts[10].pubkey,
        1,
    );
    f.external(donation).unwrap();
    f.external(create.clone()).unwrap();
    let entry: wire::Instance =
        wire::from_slice(&f.svm.get_account(&create.accounts[9].pubkey).unwrap().data).unwrap();
    assert_eq!(entry.owner_program, a.to_bytes());
    assert_eq!(entry.instance, create.accounts[1].pubkey.to_bytes());
    let duplicate = f.daily_create(b, cb, rb, 1, now);
    let loser = duplicate.accounts[1].pubkey;
    assert!(f.external(duplicate).is_err());
    assert!(f.svm.get_account(&loser).is_none());
    assert_eq!(f.state().instance_count, 1);
    f.lifecycle(
        ra,
        wire::Instruction::Discontinue {
            reason: "new release".into(),
        },
        vec![],
        false,
    )
    .unwrap();
    assert!(f
        .external(f.daily_create(a, ca, ra, 2, now + 86_400))
        .is_err());
    f.external(f.daily_create(b, cb, rb, 1, now + 86_400))
        .unwrap();
    assert_eq!(f.state().instance_count, 2);
    let mut clock = f.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = now + 2 * 86_400 + 3_601;
    f.svm.set_sysvar(&clock);
    // Existing empty round can still finish after its creation release is retired.
    f.external(Instruction {
        program_id: a,
        accounts: vec![
            M::new_readonly(ca, false),
            create.accounts[1].clone(),
            create.accounts[2].clone(),
            M::new(f.admin.pubkey(), false),
        ],
        data: vec![9],
    })
    .unwrap();
}

fn anchor_data<T: borsh::BorshSerialize>(name: &str, args: &T) -> Vec<u8> {
    let digest = solana_sha256_hasher::hash(format!("global:{name}").as_bytes());
    [digest.to_bytes()[..8].to_vec(), wire::to_vec(args).unwrap()].concat()
}
impl Fixture {
    fn external_by(
        &mut self,
        ix: Instruction,
        signer: &Keypair,
    ) -> litesvm::types::TransactionResult {
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.admin.pubkey()),
            &[&self.admin, signer],
            self.svm.latest_blockhash(),
        );
        self.submit(tx)
    }
    #[allow(clippy::too_many_arguments)]
    fn giveaway_create(
        &self,
        target: Pubkey,
        config: Pubkey,
        rel: Pubkey,
        creator: Pubkey,
        id: u64,
        nonce: u64,
        start: i64,
    ) -> Instruction {
        let giveaway = Pubkey::find_program_address(
            &[b"giveaway", config.as_ref(), &id.to_le_bytes()],
            &target,
        )
        .0;
        let vault = Pubkey::find_program_address(&[b"vault", giveaway.as_ref()], &target).0;
        let seq = (self.state().instance_count + 1).to_le_bytes();
        let key = solana_sha256_hasher::hashv(&[
            b"chance-giveaway-key-v1",
            creator.as_ref(),
            &nonce.to_le_bytes(),
        ])
        .to_bytes();
        Instruction {
            program_id: target,
            accounts: vec![
                M::new(config, false),
                M::new(giveaway, false),
                M::new(vault, false),
                M::new(creator, true),
                M::new_readonly(system::id(), false),
                M::new_readonly(self.program, false),
                M::new(self.cfg, false),
                M::new_readonly(rel, false),
                M::new_readonly(
                    Pubkey::find_program_address(&[b"chance-release", self.cfg.as_ref()], &target)
                        .0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(
                        &[b"instance", self.cfg.as_ref(), &seq],
                        &self.program,
                    )
                    .0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(
                        &[b"business", self.cfg.as_ref(), &[wire::GIVEAWAY], &key],
                        &self.program,
                    )
                    .0,
                    false,
                ),
            ],
            data: anchor_data(
                "create_giveaway_with_nonce",
                &(
                    id,
                    1_000_000u64,
                    1u32,
                    start,
                    start + 3_600,
                    3_600u32,
                    nonce,
                ),
            ),
        }
    }
}

#[test]
fn real_giveaway_creator_nonce_gate_and_provider_timeout_refund() {
    giveaway_recovery_scenario(0);
}

#[test]
fn interrupted_giveaway_finalization_refunds_at_absolute_deadline() {
    giveaway_recovery_scenario(1);
}

#[test]
fn vested_giveaway_survives_retirement_and_pays_without_provider_signature() {
    giveaway_recovery_scenario(2);
}

fn giveaway_recovery_scenario(mode: u8) {
    let mut f = Fixture::new();
    let program = solana_program::pubkey!("DUMRJ15A2ivmUNDK6EX7wfRQ1cYw4vw5ewSyT8xSJuRG");
    f.svm
        .add_program_from_file(
            program,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/deploy/giveaways.so"
            ),
        )
        .unwrap();
    let rel = f.register_domain(program, wire::GIVEAWAY);
    let cfg = Pubkey::find_program_address(&[b"config"], &program).0;
    f.external(Instruction {
        program_id: program,
        accounts: vec![
            M::new(cfg, false),
            M::new(f.admin.pubkey(), true),
            M::new_readonly(system::id(), false),
        ],
        data: anchor_data("initialize", &(500u16, 3_600u32, 3_600u32)),
    })
    .unwrap();
    f.external(Instruction {
        program_id: program,
        accounts: vec![
            M::new(cfg, false),
            M::new_readonly(f.admin.pubkey(), true),
            M::new_readonly(f.program, false),
            M::new_readonly(f.cfg, false),
            M::new_readonly(rel, false),
        ],
        data: anchor_data("bind_registry", &()),
    })
    .unwrap();
    f.advance();
    f.activate_large(rel, program);
    let creator = Keypair::new();
    f.svm.airdrop(&creator.pubkey(), 1_000_000_000).unwrap();
    let start = f.svm.get_sysvar::<Clock>().unix_timestamp;
    let create = f.giveaway_create(program, cfg, rel, creator.pubkey(), 1, 42, start);
    let mut bypass = create.clone();
    bypass.accounts.truncate(5);
    assert!(f.external_by(bypass, &creator).is_err());
    f.external_by(create.clone(), &creator).unwrap();
    assert_eq!(f.state().instance_count, 1);
    let balance = f.svm.get_account(&creator.pubkey()).unwrap().lamports;
    let repeated = f.giveaway_create(program, cfg, rel, creator.pubkey(), 2, 42, start);
    assert!(f.external_by(repeated, &creator).is_err());
    assert_eq!(
        f.svm.get_account(&creator.pubkey()).unwrap().lamports,
        balance
    );
    let giveaway = create.accounts[1].pubkey;
    let vault = create.accounts[2].pubkey;
    let participant = Keypair::new();
    f.svm.airdrop(&participant.pubkey(), 1_000_000_000).unwrap();
    let pa = Pubkey::find_program_address(
        &[
            b"participant",
            giveaway.as_ref(),
            participant.pubkey().as_ref(),
        ],
        &program,
    )
    .0;
    let commitment = solana_sha256_hasher::hash(b"timeout-phrase\x1fsalt").to_bytes();
    f.external_by(
        Instruction {
            program_id: program,
            accounts: vec![
                M::new_readonly(cfg, false),
                M::new(giveaway, false),
                M::new(pa, false),
                M::new(participant.pubkey(), true),
                M::new_readonly(system::id(), false),
            ],
            data: anchor_data("participate", &(commitment, "proof".to_string())),
        },
        &participant,
    )
    .unwrap();
    let mut additional = Vec::new();
    if mode == 2 {
        for _ in 0..2 {
            let wallet = Keypair::new();
            f.svm.airdrop(&wallet.pubkey(), 1_000_000_000).unwrap();
            let account = Pubkey::find_program_address(
                &[b"participant", giveaway.as_ref(), wallet.pubkey().as_ref()],
                &program,
            )
            .0;
            f.external_by(
                Instruction {
                    program_id: program,
                    accounts: vec![
                        M::new_readonly(cfg, false),
                        M::new(giveaway, false),
                        M::new(account, false),
                        M::new(wallet.pubkey(), true),
                        M::new_readonly(system::id(), false),
                    ],
                    data: anchor_data("participate", &(commitment, "proof".to_string())),
                },
                &wallet,
            )
            .unwrap();
            additional.push((wallet, account));
        }
    }
    let mut clock = f.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = start + 3_600;
    f.svm.set_sysvar(&clock);
    f.external_by(
        Instruction {
            program_id: program,
            accounts: vec![
                M::new_readonly(cfg, false),
                M::new(giveaway, false),
                M::new(pa, false),
                M::new_readonly(participant.pubkey(), true),
            ],
            data: anchor_data(
                "attest_reveal",
                &("timeout-phrase".to_string(), b"salt".to_vec()),
            ),
        },
        &participant,
    )
    .unwrap();
    if mode == 2 {
        let (wallet, account) = &additional[0];
        f.external_by(
            Instruction {
                program_id: program,
                accounts: vec![
                    M::new_readonly(cfg, false),
                    M::new(giveaway, false),
                    M::new(*account, false),
                    M::new_readonly(wallet.pubkey(), true),
                ],
                data: anchor_data(
                    "attest_reveal",
                    &("timeout-phrase".to_string(), b"salt".to_vec()),
                ),
            },
            wallet,
        )
        .unwrap();
        // Third participant does not attest; only the two accepted reveals are eligible.
        clock.unix_timestamp = start + 7_200;
        f.svm.set_sysvar(&clock);
    }
    let winners =
        Pubkey::find_program_address(&[b"winners_root_v2", giveaway.as_ref()], &program).0;
    let finalization =
        Pubkey::find_program_address(&[b"finalization_root_v2", giveaway.as_ref()], &program).0;
    let mut finalize = Instruction {
        program_id: program,
        accounts: vec![
            M::new_readonly(cfg, false),
            M::new(giveaway, false),
            M::new_readonly(vault, false),
            M::new(winners, false),
            M::new(finalization, false),
            M::new(f.admin.pubkey(), true),
            M::new_readonly(system::id(), false),
            M::new(pa, false),
        ],
        data: anchor_data("finalize_winners", &()),
    };
    for (_, account) in &additional {
        finalize.accounts.push(M::new(*account, false));
    }
    let mut payout_wallet = participant.pubkey();
    let mut payout_account = pa;
    if mode > 0 {
        // A valid creator is still not authorized to advance provider finalization.
        let mut unauthorized = finalize.clone();
        unauthorized.accounts[5] = M::new(creator.pubkey(), true);
        assert!(f.external_by(unauthorized, &creator).is_err());
        let mut completed = false;
        for _ in 0..if mode == 1 { 1 } else { 80 } {
            let result = f.external(finalize.clone()).unwrap();
            for line in &result.logs {
                if let Some((_, json)) = line.split_once("GIVEAWAY_EVENT: ") {
                    let event: serde_json::Value = serde_json::from_str(json).unwrap();
                    if event["event"]["event_type"] == "WinnerSelected" {
                        payout_wallet = event["event"]["data"]["participant"]
                            .as_str()
                            .unwrap()
                            .parse()
                            .unwrap();
                        payout_account = Pubkey::find_program_address(
                            &[b"participant", giveaway.as_ref(), payout_wallet.as_ref()],
                            &program,
                        )
                        .0;
                    }
                }
            }
            completed = result
                .logs
                .iter()
                .any(|line| line.contains("WinnersComputed"));
            if completed {
                break;
            }
        }
        assert_eq!(completed, mode == 2);
    }
    f.lifecycle(
        rel,
        wire::Instruction::Discontinue {
            reason: "provider incident".into(),
        },
        vec![],
        true,
    )
    .unwrap();
    let next = f.giveaway_create(
        program,
        cfg,
        rel,
        creator.pubkey(),
        2,
        43,
        clock.unix_timestamp,
    );
    assert!(f.external_by(next, &creator).is_err());
    let refund = Instruction {
        program_id: program,
        accounts: vec![
            M::new_readonly(cfg, false),
            M::new(giveaway, false),
            M::new(vault, false),
            M::new(creator.pubkey(), false),
            M::new_readonly(f.admin.pubkey(), false),
        ],
        data: anchor_data("settle_giveaway", &()),
    };
    clock.unix_timestamp = start + 10_799;
    f.svm.set_sysvar(&clock);
    assert!(f.external(refund.clone()).is_err());
    clock.unix_timestamp += 1;
    f.svm.set_sysvar(&clock);
    if mode == 2 {
        assert!(
            f.external(refund).is_err(),
            "vesting forbids timeout refund"
        );
        assert!(
            f.external(finalize).is_err(),
            "vesting cannot restart selection"
        );
        let payout = Instruction {
            program_id: program,
            accounts: vec![
                M::new_readonly(cfg, false),
                M::new(giveaway, false),
                M::new(vault, false),
                M::new(winners, false),
                M::new(f.admin.pubkey(), false),
                M::new(creator.pubkey(), false),
                M::new(payout_account, false),
                M::new(payout_wallet, false),
            ],
            // Borsh WinnerProof: wallet, amount, winner index, legacy proof vector.
            data: anchor_data(
                "settle_payout_batch",
                &(
                    0u32,
                    vec![(
                        payout_wallet.to_bytes(),
                        950_000u64,
                        0u32,
                        Vec::<Vec<u8>>::new(),
                    )],
                ),
            ),
        };
        let winner_before = f.svm.get_account(&payout_wallet).unwrap().lamports;
        let provider_before = f.svm.get_account(&f.admin.pubkey()).unwrap().lamports;
        for _ in 0..2 {
            f.svm.expire_blockhash();
            // Only the creator pays/signs; the frozen provider is a writable recipient.
            let tx = Transaction::new_signed_with_payer(
                std::slice::from_ref(&payout),
                Some(&creator.pubkey()),
                &[&creator],
                f.svm.latest_blockhash(),
            );
            f.submit(tx).unwrap();
            assert_eq!(
                f.svm.get_account(&payout_wallet).unwrap().lamports,
                winner_before + 950_000
            );
            assert_eq!(
                f.svm.get_account(&f.admin.pubkey()).unwrap().lamports,
                provider_before + 50_000
            );
        }
        assert_eq!(
            f.svm
                .get_account(&vault)
                .map_or(0, |account| account.lamports),
            0
        );
        if let Ok(path) = std::env::var("CHANCE_AUDIT_FIXTURE") {
            let inventory = Pubkey::find_program_address(
                &[b"instance", f.cfg.as_ref(), &1u64.to_le_bytes()],
                &f.program,
            )
            .0;
            let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
            let target_data = Pubkey::find_program_address(&[program.as_ref()], &loader).0;
            let registry_data = Pubkey::find_program_address(&[f.program.as_ref()], &loader).0;
            let mut accounts = serde_json::Map::new();
            for address in [
                inventory,
                f.cfg,
                rel,
                program,
                target_data,
                f.program,
                registry_data,
                giveaway,
                vault,
                winners,
            ] {
                let Some(account) = f.svm.get_account(&address) else {
                    continue;
                };
                let hex: String = account
                    .data
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                accounts.insert(address.to_string(), serde_json::json!({"owner":account.owner.to_string(), "lamports":account.lamports, "executable":account.executable, "dataHex":hex}));
            }
            for sequence in 1..=f.state().record_count {
                let address = Pubkey::find_program_address(
                    &[b"record", f.cfg.as_ref(), &sequence.to_le_bytes()],
                    &f.program,
                )
                .0;
                let account = f.svm.get_account(&address).unwrap();
                let hex: String = account
                    .data
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                accounts.insert(address.to_string(), serde_json::json!({"owner":account.owner.to_string(), "lamports":account.lamports,"executable":account.executable,"dataHex":hex}));
            }
            let fixture = serde_json::json!({"kind":"isolated LiteSVM test bank, not a network deployment", "registry":f.program.to_string(), "inventory":inventory.to_string(), "slot":clock.slot, "accounts":accounts, "history":f.history});
            std::fs::write(path, serde_json::to_vec(&fixture).unwrap()).unwrap();
        }
        return;
    }
    let escrow = f.svm.get_account(&vault).unwrap().lamports;
    let before = f.svm.get_account(&creator.pubkey()).unwrap().lamports;
    f.external(refund.clone()).unwrap();
    assert_eq!(
        f.svm.get_account(&creator.pubkey()).unwrap().lamports,
        before + escrow
    );
    f.external(refund).unwrap(); // idempotent; no second payment
    assert_eq!(
        f.svm.get_account(&creator.pubkey()).unwrap().lamports,
        before + escrow
    );
}

#[test]
fn real_daily_weighted_selection_and_public_replay_fixture() {
    let mut f = Fixture::new();
    let program = Pubkey::new_unique();
    f.svm
        .add_program_from_file(
            program,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/deploy/daily_lottery.so"
            ),
        )
        .unwrap();
    let rel = f.register(program);
    let cfg = f.daily_setup(program, rel);
    f.advance();
    f.activate_large(rel, program);
    let start = f.svm.get_sysvar::<Clock>().unix_timestamp;
    let create = f.daily_create(program, cfg, rel, 1, start);
    let lottery = create.accounts[1].pubkey;
    let vault = create.accounts[2].pubkey;
    f.external(create).unwrap();
    let plaintext = b"audit-phrase\x1fsalt".to_vec();
    let commitment = solana_sha256_hasher::hash(&plaintext).to_bytes();
    let mut participants = Vec::new();
    for tickets in [2u64, 3, 1] {
        let wallet = Keypair::new();
        f.svm.airdrop(&wallet.pubkey(), 1_000_000_000).unwrap();
        let account = Pubkey::find_program_address(
            &[b"participant", lottery.as_ref(), wallet.pubkey().as_ref()],
            &program,
        )
        .0;
        f.external_by(
            Instruction {
                program_id: program,
                accounts: vec![
                    M::new_readonly(cfg, false),
                    M::new(lottery, false),
                    M::new(vault, false),
                    M::new(account, false),
                    M::new(wallet.pubkey(), true),
                    M::new_readonly(system::id(), false),
                ],
                data: [vec![3], wire::to_vec(&(Some(commitment), tickets)).unwrap()].concat(),
            },
            &wallet,
        )
        .unwrap();
        participants.push((wallet, account));
    }
    let mut clock = f.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = start + 86_400;
    f.svm.set_sysvar(&clock);
    let tally = Pubkey::find_program_address(&[b"vote_tally", lottery.as_ref()], &program).0;
    for (wallet, account) in participants.iter().take(2) {
        f.external_by(
            Instruction {
                program_id: program,
                accounts: vec![
                    M::new_readonly(cfg, false),
                    M::new(lottery, false),
                    M::new(*account, false),
                    M::new(wallet.pubkey(), true),
                    M::new(tally, false),
                    M::new_readonly(system::id(), false),
                ],
                data: [vec![15], wire::to_vec(&(2u64, plaintext.clone())).unwrap()].concat(),
            },
            wallet,
        )
        .unwrap();
    }
    clock.unix_timestamp = start + 172_801;
    f.svm.set_sysvar(&clock);
    let root =
        Pubkey::find_program_address(&[b"finalization_root_v2", lottery.as_ref()], &program).0;
    let page = Pubkey::find_program_address(
        &[b"winner_page", lottery.as_ref(), &0u32.to_le_bytes()],
        &program,
    )
    .0;
    let mut accounts = vec![
        M::new_readonly(cfg, false),
        M::new(lottery, false),
        M::new(vault, false),
        M::new(f.admin.pubkey(), true),
        M::new_readonly(system::id(), false),
        M::new(root, false),
        M::new(page, false),
    ];
    accounts.extend(
        participants
            .iter()
            .map(|(_, account)| M::new(*account, false)),
    );
    let finalize = Instruction {
        program_id: program,
        accounts,
        data: vec![7],
    };
    let mut winners = Vec::new();
    let mut completed = false;
    for _ in 0..5 {
        let result = f.external(finalize.clone()).unwrap();
        for line in &result.logs {
            if let Some((_, json)) = line.split_once("LOTTERY_EVENT: ") {
                let event: serde_json::Value = serde_json::from_str(json).unwrap();
                if event["event"]["event_type"] == "WinnerSelected" {
                    winners.push(
                        event["event"]["data"]["winner"]
                            .as_str()
                            .unwrap()
                            .parse::<Pubkey>()
                            .unwrap(),
                    );
                }
                if event["event"]["event_type"] == "WinnersFinalized" {
                    completed = true;
                }
            }
        }
        if completed {
            break;
        }
    }
    assert!(completed);
    assert_eq!(winners.len(), 2);
    assert!(!winners.contains(&participants[2].0.pubkey()));
    let proofs: Vec<_> = winners
        .iter()
        .enumerate()
        .map(|(i, wallet)| {
            (
                i as u64,
                wallet.to_bytes(),
                2_850_000u64,
                Vec::<[u8; 32]>::new(),
            )
        })
        .collect();
    let mut accounts = vec![
        M::new_readonly(cfg, false),
        M::new(lottery, false),
        M::new(vault, false),
        M::new(f.admin.pubkey(), false),
        M::new_readonly(system::id(), false),
        M::new(page, false),
    ];
    accounts.extend(winners.iter().map(|wallet| M::new(*wallet, false)));
    f.external(Instruction {
        program_id: program,
        accounts,
        data: [vec![10], wire::to_vec(&(1u64, 0u32, proofs)).unwrap()].concat(),
    })
    .unwrap();
    if let Ok(path) = std::env::var("CHANCE_DAILY_AUDIT_FIXTURE") {
        let inventory = Pubkey::find_program_address(
            &[b"instance", f.cfg.as_ref(), &1u64.to_le_bytes()],
            &f.program,
        )
        .0;
        let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
        let target_data = Pubkey::find_program_address(&[program.as_ref()], &loader).0;
        let registry_data = Pubkey::find_program_address(&[f.program.as_ref()], &loader).0;
        let mut accounts = serde_json::Map::new();
        for address in [
            inventory,
            f.cfg,
            rel,
            program,
            target_data,
            f.program,
            registry_data,
            lottery,
            vault,
        ] {
            let Some(account) = f.svm.get_account(&address) else {
                continue;
            };
            let hex: String = account
                .data
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            accounts.insert(address.to_string(), serde_json::json!({"owner":account.owner.to_string(), "lamports":account.lamports, "executable":account.executable, "dataHex":hex}));
        }
        for sequence in 1..=f.state().record_count {
            let address = Pubkey::find_program_address(
                &[b"record", f.cfg.as_ref(), &sequence.to_le_bytes()],
                &f.program,
            )
            .0;
            let account = f.svm.get_account(&address).unwrap();
            let hex: String = account
                .data
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            accounts.insert(address.to_string(), serde_json::json!({"owner":account.owner.to_string(), "lamports":account.lamports,"executable":account.executable,"dataHex":hex}));
        }

        let fixture = serde_json::json!({"kind":"isolated LiteSVM test bank, not a network deployment", "registry":f.program.to_string(), "inventory":inventory.to_string(), "slot":clock.slot, "accounts":accounts, "history":f.history});
        std::fs::write(path, serde_json::to_vec(&fixture).unwrap()).unwrap();
    }
}
