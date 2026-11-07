//! Execute the successor ELF, not its host entrypoint or a model.
//! Run explicitly with CHANCE_GIVEAWAY_BRAND_REVIEW_ELF and --ignored; absence never passes.
#![allow(clippy::result_large_err, dead_code)]
use borsh::{BorshDeserialize, BorshSerialize};
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

const PROGRAM: Pubkey = solana_program::pubkey!("EahQmXc3rwhY3CH1g3ZgUx8L4vHTNmzpK1xtiQ1RAxq6");
const BOOTSTRAP: Pubkey = solana_program::pubkey!("Eechuq54TEzmBHXX7Ltbmpu6K5Vf3qgdsRdYmn41vJ9o");
type ResourceMetrics = std::collections::BTreeMap<[u8; 8], (u64, u64, u64)>;
static RESOURCE_METRICS: std::sync::Mutex<ResourceMetrics> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
fn h(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &PROGRAM).0
}
fn anchor<T: BorshSerialize>(name: &str, args: &T) -> Vec<u8> {
    [
        h(&[format!("global:{name}").as_bytes()])[..8].to_vec(),
        borsh::to_vec(args).unwrap(),
    ]
    .concat()
}
fn ix<T: BorshSerialize>(name: &str, args: &T, accounts: Vec<M>) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts,
        data: anchor(name, args),
    }
}
fn send(svm: &mut LiteSVM, instruction: Instruction, signer: &Keypair) -> bool {
    let discriminator_key: [u8; 8] = instruction.data[..8].try_into().unwrap();
    svm.expire_blockhash();
    let discriminator = instruction.data[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&signer.pubkey()),
        &[signer],
        svm.latest_blockhash(),
    );
    let tx_bytes = 1 + tx.signatures.len() * 64 + tx.message.serialize().len();
    assert!(
        tx_bytes <= 1232,
        "pinned legacy transaction exceeds packet limit: {tx_bytes}"
    );
    match svm.send_transaction(tx) {
        Ok(meta) => {
            if std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_FULL_POOL").is_some() {
                let mut metrics = RESOURCE_METRICS.lock().unwrap();
                let (calls, total, maximum) = metrics.entry(discriminator_key).or_default();
                *calls += 1;
                *total += meta.compute_units_consumed;
                *maximum = (*maximum).max(meta.compute_units_consumed);
            }
            eprintln!(
                "SBF consumed {} CU, {tx_bytes} transaction bytes",
                meta.compute_units_consumed
            );
            if let Ok(path) = std::env::var("CHANCE_GIVEAWAY_BRAND_REVIEW_METRICS") {
                use std::io::Write;
                let record = serde_json::json!({"discriminator":discriminator,"computeUnits":meta.compute_units_consumed,"transactionBytes":tx_bytes});
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap();
                file.write_all(format!("{record}\n").as_bytes()).unwrap();
            }
            true
        }
        Err(e) => {
            eprintln!("SBF expected/actual failure: {e:?}");
            false
        }
    }
}
fn account<T: BorshDeserialize>(svm: &LiteSVM, key: Pubkey) -> T {
    T::deserialize(&mut &svm.get_account(&key).unwrap().data[8..]).unwrap()
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn maximum_task_text_and_four_versions_seal_in_actual_sbf_packets() {
    let mut e = Env::with_tasks(1, 32, true);
    let w = Keypair::new();
    e.begin(&w);
    for task in 0..32u16 {
        for version in 1..=4u8 {
            assert!(e.publish(&w, task, version, &"e".repeat(256)));
        }
    }
    assert!(!e.publish(&w, 0, 5, "excess version"));
    e.begin_seal(&w);
    for base in (0..32u16).step_by(8) {
        let pairs = (base..base + 8).map(|t| (t, 4)).collect::<Vec<_>>();
        assert!(e.seal_chunk(&w, &pairs));
    }
    assert!(e.finish_seal(&w));
    let p: P = account(&e.svm, e.entry_key(&w));
    assert_eq!((p.current_bytes, p.revision, p.index), (8192, 129, 0));
    let evidence_rent = (0..32u16)
        .flat_map(|t| (1..=4u8).map(move |v| (t, v)))
        .map(|(t, v)| e.svm.get_balance(&e.evidence_key(&w, t, v)).unwrap())
        .sum::<u64>();
    eprintln!("MAXIMUM_RETAINED_EVIDENCE_RENT_LAMPORTS={evidence_rent}");
    e.clock(e.terms.entry_end);
    assert!(e.reveal(&w));
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts; maximum-count ranking fixture"]
fn maximum_registered_pool_and_consecutive_wave_boundary_execute_in_sbf() {
    let complete_pool = std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_FULL_POOL").is_some();
    RESOURCE_METRICS.lock().unwrap().clear();
    let started = std::time::Instant::now();
    let mut e = Env::new(1000);
    if complete_pool {
        // This pays actual permanent candidate-account rent and host execution
        // fees in the isolated benchmark, not out of the advertised prize pool.
        e.svm.airdrop(&e.host.pubkey(), 100_000_000_000).unwrap();
    }
    let host_before = e.svm.get_balance(&e.host.pubkey()).unwrap();
    let entries = resource_entries(&mut e, 4096);
    e.clock(e.terms.upload_end);
    assert!(send(
        &mut e.svm,
        ix(
            "begin_pool",
            &(),
            vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new(e.host.pubkey(), true),
                M::new_readonly(system::id(), false)
            ]
        ),
        &e.host
    ));
    for operation in ["process_pool", "process_wave"] {
        for chunk in entries.chunks(4) {
            let mut accounts = vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new_readonly(e.host.pubkey(), true),
            ];
            accounts.extend(chunk.iter().map(|(_, p)| M::new_readonly(*p, false)));
            assert!(send(&mut e.svm, ix(operation, &(), accounts), &e.host));
        }
    }
    let g: G = account(&e.svm, e.g);
    assert_eq!(g.base_pool_count, 4096);
    let mut expected = entries
        .iter()
        .map(|(wallet, p)| {
            (
                h(&[b"GIVEAWAY_RANK_V2", &g.seed, wallet.as_ref()]),
                wallet.to_bytes(),
                *p,
            )
        })
        .collect::<Vec<_>>();
    expected.sort();
    for (ordinal, (_, _, participant)) in expected.iter().take(32).enumerate() {
        let c = pda(&[b"candidate", e.g.as_ref(), &(ordinal as u64).to_le_bytes()]);
        let s = pda(&[b"winner", e.g.as_ref(), &0u32.to_le_bytes()]);
        assert!(send(
            &mut e.svm,
            ix(
                "open_candidate",
                &(ordinal as u64),
                vec![
                    M::new(e.g, false),
                    M::new(e.ranking, false),
                    M::new_readonly(*participant, false),
                    M::new(c, false),
                    M::new(s, false),
                    M::new(e.host.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            &e.host
        ));
        assert!(e.review(ordinal as u64, false, false));
    }
    assert!(send(
        &mut e.svm,
        ix(
            "next_wave",
            &(),
            vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new_readonly(e.host.pubkey(), true)
            ]
        ),
        &e.host
    ));
    for chunk in entries.chunks(4) {
        let mut accounts = vec![
            M::new(e.g, false),
            M::new(e.ranking, false),
            M::new_readonly(e.host.pubkey(), true),
        ];
        accounts.extend(chunk.iter().map(|(_, p)| M::new_readonly(*p, false)));
        assert!(send(&mut e.svm, ix("process_wave", &(), accounts), &e.host));
    }
    let c = pda(&[b"candidate", e.g.as_ref(), &32u64.to_le_bytes()]);
    let s = pda(&[b"winner", e.g.as_ref(), &0u32.to_le_bytes()]);
    assert!(send(
        &mut e.svm,
        ix(
            "open_candidate",
            &32u64,
            vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new_readonly(expected[32].2, false),
                M::new(c, false),
                M::new(s, false),
                M::new(e.host.pubkey(), true),
                M::new_readonly(system::id(), false)
            ]
        ),
        &e.host
    ));
    assert_eq!(account::<C>(&e.svm, c).wallet, expected[32].1);
    if complete_pool {
        assert!(e.review(32, false, false));
        for (ordinal, (_, wallet, participant)) in expected.iter().enumerate().skip(33) {
            if ordinal % 32 == 0 {
                assert!(send(
                    &mut e.svm,
                    ix(
                        "next_wave",
                        &(),
                        vec![
                            M::new(e.g, false),
                            M::new(e.ranking, false),
                            M::new_readonly(e.host.pubkey(), true),
                        ]
                    ),
                    &e.host
                ));
                for chunk in entries.chunks(4) {
                    let keys = chunk.iter().map(|(_, key)| *key).collect::<Vec<_>>();
                    assert!(coverage_chunk(&mut e, "process_wave", &keys));
                }
            }
            let ordinal = ordinal as u64;
            let candidate = pda(&[b"candidate", e.g.as_ref(), &ordinal.to_le_bytes()]);
            assert!(send(
                &mut e.svm,
                ix(
                    "open_candidate",
                    &ordinal,
                    vec![
                        M::new(e.g, false),
                        M::new(e.ranking, false),
                        M::new_readonly(*participant, false),
                        M::new(candidate, false),
                        M::new(s, false),
                        M::new(e.host.pubkey(), true),
                        M::new_readonly(system::id(), false),
                    ]
                ),
                &e.host
            ));
            assert_eq!(account::<C>(&e.svm, candidate).wallet, *wallet);
            assert!(e.review(ordinal, false, false));
        }
        let final_state: G = account(&e.svm, e.g);
        assert_eq!(final_state.consumed_count, 4096);
        assert_eq!(final_state.seed, g.seed);
        assert!(!final_state.pending);
        assert_eq!(final_state.approved_count, 0);
        assert!(!e.lock());
        assert!(e.refund());
        assert_eq!(account::<G>(&e.svm, e.g).fee, 0);
        let metrics = RESOURCE_METRICS.lock().unwrap();
        let mut operations = serde_json::Map::new();
        for name in [
            "process_pool",
            "process_wave",
            "open_candidate",
            "review_candidate",
            "next_wave",
        ] {
            let discriminator: [u8; 8] = h(&[format!("global:{name}").as_bytes()])[..8]
                .try_into()
                .unwrap();
            let &(calls, total, maximum) = metrics.get(&discriminator).unwrap();
            operations.insert(name.into(), serde_json::json!({"calls":calls,"totalComputeUnits":total,"maximumComputeUnits":maximum}));
        }
        let report = serde_json::json!({
            "qualification":"isolated actual SBF ranking fixture; admission and human review time are separate",
            "participants":4096,"rejectedCandidates":4096,"candidateWaves":128,
            "rankingBatch":4,"wallSeconds":started.elapsed().as_secs_f64(),
            "hostLamportsSpent":host_before-e.svm.get_balance(&e.host.pubkey()).unwrap(),
            "operations":operations,
        });
        eprintln!("FULL_POOL_RESOURCE_REPORT={report}");
        if let Some(path) = std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_FULL_POOL_REPORT") {
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
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

#[derive(Clone, BorshSerialize, BorshDeserialize)]
struct Terms {
    funding: u64,
    winners: u32,
    entry_start: i64,
    entry_end: i64,
    upload_end: i64,
    remediation_end: i64,
    opening_cutoff: i64,
    absolute_deadline: i64,
    review_seconds: u32,
    finalization_reserve_seconds: u32,
}
#[derive(Clone, BorshSerialize)]
struct Task {
    task_id: u16,
    title: String,
    instructions: String,
    target_uri: String,
    evidence_prompt: String,
    max_evidence_bytes: u16,
}
#[derive(BorshSerialize, BorshDeserialize)]
struct G {
    id: u64,
    config: [u8; 32],
    rulebook: [u8; 32],
    creator: [u8; 32],
    brand: [u8; 32],
    host: [u8; 32],
    vault: [u8; 32],
    rules_hash: [u8; 32],
    network_domain: [u8; 32],
    task_root: [u8; 32],
    task_count: u16,
    fee_bps: u16,
    terms: Terms,
    status: u8,
    registered_count: u64,
    sealed_count: u64,
    attested_count: u64,
    included_count: u64,
    aggregate: [u8; 32],
    base_pool_count: u64,
    pool_hash: [u8; 32],
    seed: [u8; 32],
    consumed_count: u64,
    approved_count: u32,
    accepted_root: [u8; 32],
    pending: bool,
    pending_deadline: i64,
    per_winner: u64,
    fee: u64,
    remainder: u64,
    outcome_hash: [u8; 32],
    paid_winners: u32,
    fee_paid: bool,
    remainder_paid: bool,
}
#[derive(BorshSerialize, BorshDeserialize)]
struct P {
    giveaway: [u8; 32],
    wallet: [u8; 32],
    index: u64,
    commitment: [u8; 32],
    revision: u32,
    versions: [u8; 32],
    lengths: [u16; 32],
    current_bytes: u32,
    sealed: bool,
    sealed_revision: u32,
    evidence_root: [u8; 32],
    attested: bool,
    revealed: bool,
    contribution: [u8; 32],
    bump: u8,
}
#[derive(BorshDeserialize)]
struct C {
    giveaway: [u8; 32],
    ordinal: u64,
    key: [u8; 64],
    wallet: [u8; 32],
    entry_revision: u32,
    evidence_root: [u8; 32],
    slot: u32,
    opened_at: i64,
    deadline: i64,
    verdict: u8,
    reason: u8,
    note: String,
    task_mask: u32,
    decided_at: i64,
}
#[derive(BorshDeserialize)]
struct W {
    giveaway: [u8; 32],
    slot: u32,
    wallet: [u8; 32],
    ordinal: u64,
    revision: u32,
    evidence_root: [u8; 32],
    approved: bool,
    paid: bool,
}
struct Env {
    svm: LiteSVM,
    host: Keypair,
    brand: Keypair,
    outsider: Keypair,
    config: Pubkey,
    rules: Pubkey,
    g: Pubkey,
    vault: Pubkey,
    ranking: Pubkey,
    tasks: Vec<Pubkey>,
    terms: Terms,
    registry: Pubkey,
    registry_config: Pubkey,
    release: Pubkey,
    registry_entry: Pubkey,
    business: Pubkey,
}
impl Env {
    fn new(winners: u32) -> Self {
        Self::with_tasks(winners, 2, false)
    }
    fn with_tasks(winners: u32, task_count: u16, maximum_text: bool) -> Self {
        let mut svm = LiteSVM::new();
        // Keep fixture registration slot <= actual registry creation slot.
        svm.warp_to_slot(10);
        svm.add_program_from_file(
            PROGRAM,
            std::env::var("CHANCE_GIVEAWAY_BRAND_REVIEW_ELF").expect("explicit successor ELF required"),
        )
        .unwrap();
        let registry = Pubkey::new_unique();
        svm.add_program_from_file(
            registry,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/deploy/chance_registry.so"
            ),
        )
        .unwrap();
        let host = Keypair::new();
        let brand = Keypair::new();
        let outsider = Keypair::new();
        for who in [BOOTSTRAP, host.pubkey(), brand.pubkey(), outsider.pubkey()] {
            svm.airdrop(&who, 10_000_000_000).unwrap();
        }
        let governance = Pubkey::new_unique();
        let registry_config =
            Pubkey::find_program_address(&[b"registry", governance.as_ref()], &registry).0;
        let release = Pubkey::find_program_address(
            &[b"release", registry_config.as_ref(), PROGRAM.as_ref()],
            &registry,
        )
        .0;
        let cfg = wire::Config {
            tag: wire::CONFIG_TAG,
            schema: wire::SCHEMA,
            authority: governance.to_bytes(),
            guardian: Pubkey::new_unique().to_bytes(),
            activation_delay: 1,
            release_count: 1,
            record_count: 1,
            instance_count: 0,
        };
        let rel = wire::Release {
            tag: wire::RELEASE_TAG,
            registry: registry_config.to_bytes(),
            program: PROGRAM.to_bytes(),
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
        svm.set_account(
            registry_config,
            stored(wire::to_vec(&cfg).unwrap(), registry),
        )
        .unwrap();
        svm.set_account(release, stored(wire::to_vec(&rel).unwrap(), registry))
            .unwrap();
        let config = pda(&[b"config"]);
        let init = ix(
            "initialize",
            &(host.pubkey().to_bytes(), 500u16, [7u8; 32]),
            vec![
                M::new(config, false),
                M::new(BOOTSTRAP, true),
                M::new_readonly(system::id(), false),
            ],
        );
        // The bootstrap key never enters tests. Only this isolated setup disables
        // signature cryptography; all actual user/host/brand tests restore it.
        svm = svm.with_sigverify(false);
        let mut tx = Transaction::new_with_payer(&[init], Some(&BOOTSTRAP));
        tx.message.recent_blockhash = svm.latest_blockhash();
        svm.send_transaction(tx).unwrap();
        svm = svm.with_sigverify(true);
        assert!(send(
            &mut svm,
            ix(
                "bind_registry",
                &(),
                vec![
                    M::new(config, false),
                    M::new_readonly(host.pubkey(), true),
                    M::new_readonly(registry, false),
                    M::new_readonly(registry_config, false),
                    M::new_readonly(release, false)
                ]
            ),
            &host
        ));
        let rules = pda(&[b"rules", brand.pubkey().as_ref(), &42u64.to_le_bytes()]);
        assert!(send(
            &mut svm,
            ix(
                "begin_rulebook",
                &(42u64, task_count),
                vec![
                    M::new(rules, false),
                    M::new(brand.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            &brand
        ));
        let mut tasks = Vec::new();
        let mut root = [0; 32];
        for task_id in 0..task_count {
            let task = Task {
                task_id,
                title: if maximum_text {
                    "t".repeat(64)
                } else {
                    format!("Task {task_id}")
                },
                instructions: if maximum_text {
                    "i".repeat(256)
                } else {
                    "Complete this public brand task".into()
                },
                target_uri: if maximum_text {
                    "u".repeat(128)
                } else {
                    String::new()
                },
                evidence_prompt: if maximum_text {
                    "p".repeat(128)
                } else {
                    "Public evidence".into()
                },
                max_evidence_bytes: 256,
            };
            let key = pda(&[b"task", rules.as_ref(), &task_id.to_le_bytes()]);
            root = h(&[
                b"GIVEAWAY_TASKS_V2",
                &root,
                &h(&[b"GIVEAWAY_TASK_V2", &borsh::to_vec(&task).unwrap()]),
            ]);
            assert!(send(
                &mut svm,
                ix(
                    "append_task",
                    &task,
                    vec![
                        M::new(rules, false),
                        M::new(key, false),
                        M::new(brand.pubkey(), true),
                        M::new_readonly(system::id(), false)
                    ]
                ),
                &brand
            ));
            tasks.push(key);
        }
        assert!(send(
            &mut svm,
            ix(
                "seal_rulebook",
                &(),
                vec![M::new(rules, false), M::new_readonly(brand.pubkey(), true)]
            ),
            &brand
        ));
        let now = svm.get_sysvar::<Clock>().unix_timestamp;
        let terms = Terms {
            funding: 100_003,
            winners,
            entry_start: now,
            entry_end: now + 100,
            upload_end: now + 200,
            remediation_end: now + 300,
            opening_cutoff: now + 10_000,
            absolute_deadline: now + 20_000,
            review_seconds: 100,
            finalization_reserve_seconds: 100,
        };
        let g = pda(&[b"giveaway", config.as_ref(), &1u64.to_le_bytes()]);
        let vault = pda(&[b"vault", g.as_ref()]);
        let ranking = pda(&[b"ranking", g.as_ref()]);
        let expected = h(&[
            b"GIVEAWAY_CREATION_V2",
            PROGRAM.as_ref(),
            config.as_ref(),
            registry.as_ref(),
            registry_config.as_ref(),
            host.pubkey().as_ref(),
            &[7u8; 32],
            &500u16.to_le_bytes(),
            brand.pubkey().as_ref(),
            rules.as_ref(),
            &42u64.to_le_bytes(),
            &root,
            &task_count.to_le_bytes(),
            &1u64.to_le_bytes(),
            &borsh::to_vec(&terms).unwrap(),
        ]);
        let business_hash = h(&[
            b"chance-giveaway-key-v1",
            brand.pubkey().as_ref(),
            &42u64.to_le_bytes(),
        ]);
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
            &[b"instance", registry_config.as_ref(), &1u64.to_le_bytes()],
            &registry,
        )
        .0;
        let accounts = vec![
            M::new(config, false),
            M::new(rules, false),
            M::new(g, false),
            M::new(vault, false),
            M::new(brand.pubkey(), true),
            M::new_readonly(system::id(), false),
            M::new_readonly(registry, false),
            M::new(registry_config, false),
            M::new_readonly(release, false),
            M::new_readonly(pda(&[b"chance-release", registry_config.as_ref()]), false),
            M::new(entry, false),
            M::new(business, false),
        ];
        assert!(!send(
            &mut svm,
            ix(
                "create_checked",
                &(1u64, terms.clone(), [0u8; 32]),
                accounts.clone()
            ),
            &brand
        ));
        assert!(svm.get_account(&g).is_none());
        assert!(send(
            &mut svm,
            ix("create_checked", &(1u64, terms.clone(), expected), accounts),
            &brand
        ));
        let record: wire::Instance =
            wire::from_slice(&svm.get_account(&entry).unwrap().data).unwrap();
        assert_eq!(record.rules_hash, expected);
        Self {
            svm,
            host,
            brand,
            outsider,
            config,
            rules,
            g,
            vault,
            ranking,
            tasks,
            terms,
            registry,
            registry_config,
            release,
            registry_entry: entry,
            business,
        }
    }
    fn clock(&mut self, now: i64) {
        let mut c = self.svm.get_sysvar::<Clock>();
        c.unix_timestamp = now;
        self.svm.set_sysvar(&c);
    }
    fn entry_key(&self, w: &Keypair) -> Pubkey {
        pda(&[b"participant", self.g.as_ref(), w.pubkey().as_ref()])
    }
    fn evidence_key(&self, w: &Keypair, task: u16, version: u8) -> Pubkey {
        pda(&[
            b"evidence",
            self.entry_key(w).as_ref(),
            &task.to_le_bytes(),
            &[version],
        ])
    }
    fn begin(&mut self, w: &Keypair) {
        self.svm.airdrop(&w.pubkey(), 1_000_000_000).unwrap();
        let p = self.entry_key(w);
        assert!(send(
            &mut self.svm,
            ix(
                "begin_entry",
                &h(&[b"phrase", &[31], b"salt"]),
                vec![
                    M::new(self.g, false),
                    M::new(p, false),
                    M::new(w.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            w
        ));
    }
    fn publish(&mut self, w: &Keypair, task: u16, version: u8, text: &str) -> bool {
        let p = self.entry_key(w);
        let e = self.evidence_key(w, task, version);
        send(
            &mut self.svm,
            ix(
                "publish_evidence",
                &(task, version, text.to_string()),
                vec![
                    M::new(self.g, false),
                    M::new(p, false),
                    M::new_readonly(self.tasks[usize::from(task)], false),
                    M::new(e, false),
                    M::new(w.pubkey(), true),
                    M::new_readonly(system::id(), false),
                ],
            ),
            w,
        )
    }
    fn begin_seal(&mut self, w: &Keypair) {
        let p = self.entry_key(w);
        let session = pda(&[b"entrySeal", p.as_ref()]);
        assert!(send(
            &mut self.svm,
            ix(
                "begin_entry_seal",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new(p, false),
                    M::new(session, false),
                    M::new(w.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            w
        ));
    }
    fn seal_chunk(&mut self, w: &Keypair, tasks: &[(u16, u8)]) -> bool {
        let p = self.entry_key(w);
        let session = pda(&[b"entrySeal", p.as_ref()]);
        let mut accounts = vec![
            M::new(self.g, false),
            M::new(p, false),
            M::new(session, false),
            M::new_readonly(w.pubkey(), true),
        ];
        accounts.extend(
            tasks
                .iter()
                .map(|(t, v)| M::new_readonly(self.evidence_key(w, *t, *v), false)),
        );
        send(&mut self.svm, ix("process_entry_seal", &(), accounts), w)
    }
    fn finish_seal(&mut self, w: &Keypair) -> bool {
        let p = self.entry_key(w);
        let session = pda(&[b"entrySeal", p.as_ref()]);
        send(
            &mut self.svm,
            ix(
                "finish_entry_seal",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new(p, false),
                    M::new(session, false),
                    M::new_readonly(w.pubkey(), true),
                ],
            ),
            w,
        )
    }
    fn admit(&mut self, w: &Keypair) {
        self.begin(w);
        assert!(self.publish(w, 0, 1, "handle"));
        assert!(self.publish(w, 1, 1, "public link"));
        self.begin_seal(w);
        assert!(self.seal_chunk(w, &[(0, 1), (1, 1)]));
        assert!(self.finish_seal(w));
    }
    fn reveal(&mut self, w: &Keypair) -> bool {
        let p = self.entry_key(w);
        send(
            &mut self.svm,
            ix(
                "reveal",
                &("phrase".to_string(), b"salt".to_vec()),
                vec![
                    M::new(self.g, false),
                    M::new(p, false),
                    M::new_readonly(w.pubkey(), true),
                ],
            ),
            w,
        )
    }
    fn pool(&mut self, wallets: &[Keypair]) {
        self.clock(self.terms.upload_end);
        assert!(send(
            &mut self.svm,
            ix(
                "begin_pool",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new(self.ranking, false),
                    M::new(self.host.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            &self.host
        ));
        let mut accounts = vec![
            M::new(self.g, false),
            M::new(self.ranking, false),
            M::new_readonly(self.host.pubkey(), true),
        ];
        accounts.extend(
            wallets
                .iter()
                .map(|w| M::new_readonly(self.entry_key(w), false)),
        );
        assert!(send(
            &mut self.svm,
            ix("process_pool", &(), accounts.clone()),
            &self.host
        ));
        assert!(send(
            &mut self.svm,
            ix("process_wave", &(), accounts),
            &self.host
        ));
    }
    fn order(&self, wallets: &[Keypair]) -> Vec<usize> {
        let g: G = account(&self.svm, self.g);
        let mut pairs = wallets
            .iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    h(&[b"GIVEAWAY_RANK_V2", &g.seed, w.pubkey().as_ref()]),
                    w.pubkey().to_bytes(),
                    i,
                )
            })
            .collect::<Vec<_>>();
        pairs.sort();
        pairs.into_iter().map(|(_, _, i)| i).collect()
    }
    fn open(&mut self, w: &Keypair, ordinal: u64) -> bool {
        let c = pda(&[b"candidate", self.g.as_ref(), &ordinal.to_le_bytes()]);
        let g: G = account(&self.svm, self.g);
        let s = pda(&[b"winner", self.g.as_ref(), &g.approved_count.to_le_bytes()]);
        let p = self.entry_key(w);
        send(
            &mut self.svm,
            ix(
                "open_candidate",
                &ordinal,
                vec![
                    M::new(self.g, false),
                    M::new(self.ranking, false),
                    M::new_readonly(p, false),
                    M::new(c, false),
                    M::new(s, false),
                    M::new(self.host.pubkey(), true),
                    M::new_readonly(system::id(), false),
                ],
            ),
            &self.host,
        )
    }
    fn review(&mut self, ordinal: u64, approve: bool, as_host: bool) -> bool {
        let c = pda(&[b"candidate", self.g.as_ref(), &ordinal.to_le_bytes()]);
        let candidate: C = account(&self.svm, c);
        let s = pda(&[b"winner", self.g.as_ref(), &candidate.slot.to_le_bytes()]);
        let signer = if as_host { &self.host } else { &self.brand };
        send(
            &mut self.svm,
            ix(
                "review_candidate",
                &(
                    approve,
                    if approve { 0u8 } else { 1u8 },
                    String::new(),
                    0u32,
                ),
                vec![
                    M::new(self.g, false),
                    M::new(c, false),
                    M::new(s, false),
                    M::new_readonly(signer.pubkey(), true),
                ],
            ),
            signer,
        )
    }
    fn lock(&mut self) -> bool {
        send(
            &mut self.svm,
            ix(
                "commit_outcome",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new_readonly(self.host.pubkey(), true),
                ],
            ),
            &self.host,
        )
    }
    fn refund(&mut self) -> bool {
        send(
            &mut self.svm,
            ix(
                "finalize_refund",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new(self.vault, false),
                    M::new(self.brand.pubkey(), false),
                ],
            ),
            &self.outsider,
        )
    }
    fn pay(&mut self, slot: u32, recipient: Pubkey) -> bool {
        let s = pda(&[b"winner", self.g.as_ref(), &slot.to_le_bytes()]);
        send(
            &mut self.svm,
            ix(
                "pay_winner",
                &(),
                vec![
                    M::new(self.g, false),
                    M::new(self.vault, false),
                    M::new(s, false),
                    M::new(recipient, false),
                ],
            ),
            &self.outsider,
        )
    }
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn task_completeness_revisions_and_draft_admission_execute_in_sbf() {
    let mut e = Env::new(2);
    let w = Keypair::new();
    e.begin(&w);
    let g: G = account(&e.svm, e.g);
    assert_eq!(g.registered_count, 0);
    let p: P = account(&e.svm, e.entry_key(&w));
    assert_eq!(p.index, u64::MAX);
    assert!(!e.publish(&w, 0, 1, " \n\t"));
    assert!(e.publish(&w, 0, 1, "handle"));
    e.begin_seal(&w);
    assert!(e.seal_chunk(&w, &[(0, 1)]));
    assert!(!e.finish_seal(&w));
    assert!(e.publish(&w, 1, 1, "link"));
    assert!(!e.seal_chunk(&w, &[(1, 1)]));
    e.begin_seal(&w);
    assert!(!e.seal_chunk(&w, &[(0, 1), (0, 1)]));
    assert!(e.seal_chunk(&w, &[(0, 1), (1, 1)]));
    assert!(e.finish_seal(&w));
    assert_eq!(account::<G>(&e.svm, e.g).registered_count, 1);
    assert_eq!(account::<P>(&e.svm, e.entry_key(&w)).index, 0);
    assert!(e.publish(&w, 0, 2, "updated"));
    assert!(!account::<P>(&e.svm, e.entry_key(&w)).sealed);
    assert_eq!(account::<G>(&e.svm, e.g).sealed_count, 0);
    e.clock(e.terms.entry_end);
    assert!(!e.reveal(&w));
    assert!(!e.publish(&w, 1, 2, "late"));
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn repeated_rejections_follow_fixed_order_and_only_accepted_slots_pay() {
    let mut e = Env::new(2);
    let wallets = (0..4).map(|_| Keypair::new()).collect::<Vec<_>>();
    for w in &wallets {
        e.admit(w);
    }
    e.clock(e.terms.entry_end);
    for w in &wallets {
        assert!(e.reveal(w));
    }
    e.pool(&wallets);
    let seed = account::<G>(&e.svm, e.g).seed;
    let order = e.order(&wallets);
    assert!(!e.open(&wallets[order[1]], 0));
    for (ordinal, index) in order.iter().enumerate() {
        assert!(e.open(&wallets[*index], ordinal as u64));
        assert!(!e.review(ordinal as u64, true, true));
        assert!(e.review(ordinal as u64, ordinal % 2 == 1, false));
        assert_eq!(account::<G>(&e.svm, e.g).seed, seed);
    }
    assert!(!e.pay(0, wallets[order[1]].pubkey()));
    assert!(e.lock());
    assert!(!e.refund());
    e.clock(e.terms.absolute_deadline + 1);
    assert!(!e.pay(0, wallets[order[0]].pubkey()));
    for (slot, index) in [(0, order[1]), (1, order[3])] {
        let before = e.svm.get_balance(&wallets[index].pubkey()).unwrap();
        assert!(e.pay(slot, wallets[index].pubkey()));
        assert!(e.pay(slot, wallets[index].pubkey()));
        assert_eq!(
            e.svm.get_balance(&wallets[index].pubkey()).unwrap() - before,
            47_501
        );
    }
    assert!(send(
        &mut e.svm,
        ix(
            "pay_beneficiaries",
            &(),
            vec![
                M::new(e.g, false),
                M::new(e.vault, false),
                M::new(e.host.pubkey(), false),
                M::new(e.brand.pubkey(), false)
            ]
        ),
        &e.outsider
    ));
    let g: G = account(&e.svm, e.g);
    assert_eq!(
        (g.status, g.paid_winners, g.fee, g.remainder),
        (3, 2, 5_000, 1)
    );
    export_capture(&e, &wallets);
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn cutoff_preserves_pending_full_window_then_silence_refunds_provisional_awards() {
    let mut e = Env::new(2);
    let wallets = (0..2).map(|_| Keypair::new()).collect::<Vec<_>>();
    for w in &wallets {
        e.admit(w);
    }
    e.clock(e.terms.entry_end);
    for w in &wallets {
        assert!(e.reveal(w));
    }
    e.pool(&wallets);
    let order = e.order(&wallets);
    assert!(e.open(&wallets[order[0]], 0));
    assert!(e.review(0, true, false));
    e.clock(e.terms.opening_cutoff - 50);
    assert!(e.open(&wallets[order[1]], 1));
    e.clock(e.terms.opening_cutoff);
    assert!(!e.refund());
    e.clock(e.terms.opening_cutoff + 50);
    let before = e.svm.get_balance(&e.brand.pubkey()).unwrap();
    assert!(e.refund());
    assert_eq!(
        e.svm.get_balance(&e.brand.pubkey()).unwrap() - before,
        e.terms.funding
    );
    assert!(!e.pay(0, wallets[order[0]].pubkey()));
    assert!(!e.lock());
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn full_exhaustion_shortfall_redistributes_and_all_rejected_refunds_without_fee() {
    for approve_first in [false, true] {
        let mut e = Env::new(2);
        let wallets = (0..2).map(|_| Keypair::new()).collect::<Vec<_>>();
        for w in &wallets {
            e.admit(w);
        }
        e.clock(e.terms.entry_end);
        for w in &wallets {
            assert!(e.reveal(w));
        }
        e.pool(&wallets);
        let order = e.order(&wallets);
        for (ordinal, index) in order.iter().enumerate() {
            assert!(e.open(&wallets[*index], ordinal as u64));
            assert!(e.review(ordinal as u64, approve_first && ordinal == 0, false));
        }
        if approve_first {
            assert!(e.lock());
            assert_eq!(account::<G>(&e.svm, e.g).per_winner, 95_003);
        } else {
            assert!(!e.lock());
            assert!(e.refund());
            assert_eq!(account::<G>(&e.svm, e.g).fee, 0);
        }
    }
}

fn send_bundle(svm: &mut LiteSVM, instructions: &[Instruction], payer: &Keypair) -> bool {
    svm.expire_blockhash();
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );
    match svm.send_transaction(transaction) {
        Ok(metadata) => {
            eprintln!(
                "SBF receipt consumed {} CU",
                metadata.compute_units_consumed
            );
            true
        }
        Err(error) => {
            eprintln!("SBF receipt failure: {error:?}");
            false
        }
    }
}

fn receipt(e: &mut Env, wallet: &Keypair, network_domain: [u8; 32]) -> bool {
    let participant = e.entry_key(wallet);
    let state: P = account(&e.svm, participant);
    let message = [
        b"GIVEAWAY_RECEIPT_V2".as_slice(),
        PROGRAM.as_ref(),
        &network_domain,
        e.g.as_ref(),
        wallet.pubkey().as_ref(),
        &state.commitment,
        &state.sealed_revision.to_le_bytes(),
        &state.evidence_root,
    ]
    .concat();
    let signature = e.host.sign_message(&message);
    let mut payload = vec![1u8, 0];
    for offset in [
        16u16,
        u16::MAX,
        80,
        u16::MAX,
        112,
        message.len() as u16,
        u16::MAX,
    ] {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.extend_from_slice(signature.as_ref());
    payload.extend_from_slice(e.host.pubkey().as_ref());
    payload.extend_from_slice(&message);
    let check_signature = Instruction {
        program_id: solana_program::ed25519_program::ID,
        accounts: vec![],
        data: payload,
    };
    let acknowledge = ix(
        "attest_receipt",
        &(),
        vec![
            M::new(e.g, false),
            M::new(participant, false),
            M::new_readonly(solana_program::sysvar::instructions::ID, false),
        ],
    );
    send_bundle(&mut e.svm, &[check_signature, acknowledge], &e.outsider)
}

fn relay_reveal(e: &mut Env, wallet: &Keypair, as_host: bool) -> bool {
    let participant = e.entry_key(wallet);
    let actor = if as_host { &e.host } else { &e.outsider };
    send(
        &mut e.svm,
        ix(
            "reveal",
            &("phrase".to_string(), b"salt".to_vec()),
            vec![
                M::new(e.g, false),
                M::new(participant, false),
                M::new_readonly(actor.pubkey(), true),
            ],
        ),
        actor,
    )
}

fn begin_pool(e: &mut Env) -> bool {
    send(
        &mut e.svm,
        ix(
            "begin_pool",
            &(),
            vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new(e.host.pubkey(), true),
                M::new_readonly(system::id(), false),
            ],
        ),
        &e.host,
    )
}

fn coverage_chunk(e: &mut Env, name: &str, participants: &[Pubkey]) -> bool {
    let mut accounts = vec![
        M::new(e.g, false),
        M::new(e.ranking, false),
        M::new_readonly(e.host.pubkey(), true),
    ];
    accounts.extend(participants.iter().map(|key| M::new_readonly(*key, false)));
    send(&mut e.svm, ix(name, &(), accounts), &e.host)
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn receipts_bind_network_and_early_host_publication_preserves_the_reveal_window() {
    let mut e = Env::new(2);
    let wallets = (0..2).map(|_| Keypair::new()).collect::<Vec<_>>();
    for wallet in &wallets {
        e.admit(wallet);
    }
    e.clock(e.terms.entry_end);
    assert!(!receipt(&mut e, &wallets[0], [9; 32]));
    assert!(!account::<P>(&e.svm, e.entry_key(&wallets[0])).attested);
    assert!(receipt(&mut e, &wallets[0], [7; 32]));
    assert!(!receipt(&mut e, &wallets[0], [7; 32]));
    assert!(!relay_reveal(&mut e, &wallets[0], true));
    assert!(!relay_reveal(&mut e, &wallets[0], false));
    assert!(e.reveal(&wallets[1]));
    assert!(relay_reveal(&mut e, &wallets[0], true));
    assert_eq!(account::<G>(&e.svm, e.g).included_count, 2);
    assert!(!begin_pool(&mut e));
    e.pool(&wallets);
    assert_eq!(account::<G>(&e.svm, e.g).base_pool_count, 2);
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn empty_pool_cannot_disable_the_no_attesters_refund() {
    // Cover both an entirely empty campaign and a complete entrant who never
    // attests. Neither permits the host to revoke an already available refund.
    for has_entry in [false, true] {
        let mut e = Env::new(1);
        if has_entry {
            e.admit(&Keypair::new());
        }
        e.clock(e.terms.upload_end);
        let before = e.svm.get_balance(&e.brand.pubkey()).unwrap();
        assert!(!begin_pool(&mut e));
        let state: G = account(&e.svm, e.g);
        assert_eq!(state.status, 0);
        assert_eq!(state.attested_count, 0);
        assert!(e.svm.get_account(&e.ranking).is_none());
        assert!(e.refund());
        assert_eq!(
            e.svm.get_balance(&e.brand.pubkey()).unwrap() - before,
            e.terms.funding
        );
        assert_eq!(account::<G>(&e.svm, e.g).fee, 0);
    }
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn omitted_accepted_reveals_remediate_or_refund_without_partial_pool_settlement() {
    for remediate_all in [false, true] {
        let mut e = Env::new(1);
        let wallets = (0..2).map(|_| Keypair::new()).collect::<Vec<_>>();
        for wallet in &wallets {
            e.admit(wallet);
        }
        e.clock(e.terms.entry_end);
        for wallet in &wallets {
            assert!(receipt(&mut e, wallet, [7; 32]));
        }
        assert!(!relay_reveal(&mut e, &wallets[0], false));
        e.clock(e.terms.upload_end);
        assert!(!begin_pool(&mut e));
        assert!(relay_reveal(&mut e, &wallets[0], false));
        assert!(!begin_pool(&mut e));
        if remediate_all {
            assert!(relay_reveal(&mut e, &wallets[1], false));
            e.pool(&wallets);
            assert_eq!(account::<G>(&e.svm, e.g).base_pool_count, 2);
        } else {
            assert!(!e.refund());
            e.clock(e.terms.remediation_end);
            assert!(!relay_reveal(&mut e, &wallets[1], false));
            let before = e.svm.get_balance(&e.brand.pubkey()).unwrap();
            assert!(e.refund());
            assert_eq!(
                e.svm.get_balance(&e.brand.pubkey()).unwrap() - before,
                e.terms.funding
            );
            assert_eq!(account::<G>(&e.svm, e.g).fee, 0);
            assert!(!e.lock());
        }
    }
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts"]
fn canonical_pool_scans_cover_incomplete_registered_entries_without_gaps_or_replay() {
    let mut e = Env::new(1);
    let wallets = (0..3).map(|_| Keypair::new()).collect::<Vec<_>>();
    for wallet in &wallets {
        e.admit(wallet);
    }
    // Index zero remains registered, but its latest revision is incomplete.
    assert!(e.publish(&wallets[0], 0, 2, "new evidence not resealed"));
    e.clock(e.terms.entry_end);
    assert!(!e.reveal(&wallets[0]));
    assert!(e.reveal(&wallets[1]));
    assert!(e.reveal(&wallets[2]));
    e.clock(e.terms.upload_end);
    assert!(begin_pool(&mut e));
    let keys = wallets
        .iter()
        .map(|wallet| e.entry_key(wallet))
        .collect::<Vec<_>>();
    assert!(!coverage_chunk(&mut e, "process_pool", &[keys[1]]));
    assert!(!coverage_chunk(&mut e, "process_pool", &[keys[0], keys[0]]));
    assert!(coverage_chunk(&mut e, "process_pool", &[keys[0]]));
    assert!(!coverage_chunk(&mut e, "process_pool", &[keys[0]]));
    assert!(coverage_chunk(&mut e, "process_pool", &keys[1..]));
    assert_eq!(account::<G>(&e.svm, e.g).base_pool_count, 2);
    assert!(!coverage_chunk(&mut e, "process_wave", &[keys[1]]));
    assert!(coverage_chunk(&mut e, "process_wave", &keys));
    assert!(!e.open(&wallets[0], 0));
    let order = e.order(&wallets);
    let first_valid = order.into_iter().find(|index| *index != 0).unwrap();
    assert!(e.open(&wallets[first_valid], 0));
}

fn base64_data(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[((bits >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((bits >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((bits >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(bits & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn export_capture(e: &Env, wallets: &[Keypair]) {
    let Some(path) = std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE") else {
        return;
    };
    let state: G = account(&e.svm, e.g);
    let mut keys = std::collections::BTreeSet::from([
        PROGRAM,
        e.registry,
        e.registry_config,
        e.release,
        e.registry_entry,
        e.business,
        e.config,
        e.rules,
        e.g,
        e.vault,
        e.ranking,
        e.host.pubkey(),
        e.brand.pubkey(),
        e.outsider.pubkey(),
    ]);
    keys.extend(e.tasks.iter().copied());
    for program in [PROGRAM, e.registry] {
        if let Some(value) = e.svm.get_account(&program) {
            if value.data.len() >= 36 && value.data[..4] == 2u32.to_le_bytes() {
                keys.insert(Pubkey::new_from_array(
                    value.data[4..36].try_into().unwrap(),
                ));
            }
        }
    }
    for wallet in wallets {
        keys.insert(wallet.pubkey());
        let participant = e.entry_key(wallet);
        keys.insert(participant);
        let entry: P = account(&e.svm, participant);
        for task in 0..e.tasks.len() {
            for version in 1..=entry.versions[task] {
                keys.insert(e.evidence_key(wallet, task as u16, version));
            }
        }
    }
    for ordinal in 0..state.consumed_count {
        keys.insert(pda(&[b"candidate", e.g.as_ref(), &ordinal.to_le_bytes()]));
    }
    for slot in 0..state.approved_count {
        keys.insert(pda(&[b"winner", e.g.as_ref(), &slot.to_le_bytes()]));
    }
    let mut accounts = serde_json::Map::new();
    for key in keys {
        let value = e.svm.get_account(&key).expect("capture account must exist");
        accounts.insert(
            key.to_string(),
            serde_json::json!({
                "owner": value.owner.to_string(), "lamports": value.lamports,
                "executable": value.executable, "rentEpoch": value.rent_epoch,
                "data": [base64_data(&value.data), "base64"],
            }),
        );
    }
    let elf = std::fs::read(std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_ELF").unwrap()).unwrap();
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let clock = e.svm.get_sysvar::<Clock>();
    let document = serde_json::json!({
        "schema": "giveaway-v2-runtime-capture",
        "provenance": "isolated actual ELF LiteSVM execution; not network or finality evidence",
        "program": PROGRAM.to_string(), "instance": e.g.to_string(),
        "registry": e.registry.to_string(), "registryEntry": e.registry_entry.to_string(),
        "slot": clock.slot, "clockTimestamp": clock.unix_timestamp,
        "networkDomain": hex(&state.network_domain), "programElfSha256": hex(&h(&[&elf])),
        "accounts": accounts,
    });
    std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

fn resource_entries(e: &mut Env, count: u64) -> Vec<(Pubkey, Pubkey)> {
    let mut g: G = account(&e.svm, e.g);
    let mut entries = Vec::new();
    g.registered_count = count;
    g.sealed_count = count;
    g.attested_count = count;
    g.included_count = count;
    // Resource fixture only: inject well-formed post-reveal participant state to
    // measure maximum indexed scans, not to substitute for the real admission
    // and reveal paths exercised by the other tests above.
    for index in 0..count {
        let wallet = Pubkey::new_unique();
        let (key, bump) = Pubkey::find_program_address(
            &[b"participant", e.g.as_ref(), wallet.as_ref()],
            &PROGRAM,
        );
        let contribution = h(&[
            b"GIVEAWAY_PHRASE_V2",
            wallet.as_ref(),
            &7u32.to_le_bytes(),
            b"fixture",
        ]);
        for (a, b) in g.aggregate.iter_mut().zip(contribution) {
            *a ^= b;
        }
        let p = P {
            giveaway: e.g.to_bytes(),
            wallet: wallet.to_bytes(),
            index,
            commitment: h(&[b"fixture", &[31], b"salt"]),
            revision: 3,
            versions: {
                let mut a = [0; 32];
                a[..2].fill(1);
                a
            },
            lengths: {
                let mut a = [0; 32];
                a[..2].fill(1);
                a
            },
            current_bytes: 2,
            sealed: true,
            sealed_revision: 3,
            evidence_root: h(&[b"resource-fixture", &index.to_le_bytes()]),
            attested: true,
            revealed: true,
            contribution,
            bump,
        };
        let data = [
            h(&[b"account:Participant"])[..8].to_vec(),
            borsh::to_vec(&p).unwrap(),
        ]
        .concat();
        e.svm.set_account(key, stored(data, PROGRAM)).unwrap();
        entries.push((wallet, key));
    }
    let mut data = [
        h(&[b"account:Giveaway"])[..8].to_vec(),
        borsh::to_vec(&g).unwrap(),
    ]
    .concat();
    data.resize(1024, 0);
    e.svm.set_account(e.g, stored(data, PROGRAM)).unwrap();
    entries
}

#[test]
#[ignore = "requires actual successor and registry SBF artifacts; maximum accepted-slot resource fixture"]
fn maximum_thousand_accepted_slots_vest_in_constant_work_and_pay_after_deadline() {
    RESOURCE_METRICS.lock().unwrap().clear();
    let started = std::time::Instant::now();
    let mut e = Env::new(1000);
    e.svm.airdrop(&e.host.pubkey(), 100_000_000_000).unwrap();
    let host_before = e.svm.get_balance(&e.host.pubkey()).unwrap();
    let entries = resource_entries(&mut e, 1000);
    // Real system destinations exist independently of the injected participant
    // headers, so the final payout executes actual balance transitions.
    for (wallet, _) in &entries {
        e.svm.airdrop(wallet, 1_000_000).unwrap();
    }
    e.clock(e.terms.upload_end);
    assert!(send(
        &mut e.svm,
        ix(
            "begin_pool",
            &(),
            vec![
                M::new(e.g, false),
                M::new(e.ranking, false),
                M::new(e.host.pubkey(), true),
                M::new_readonly(system::id(), false)
            ]
        ),
        &e.host
    ));
    for chunk in entries.chunks(4) {
        let keys = chunk.iter().map(|(_, key)| *key).collect::<Vec<_>>();
        assert!(coverage_chunk(&mut e, "process_pool", &keys));
    }
    let g: G = account(&e.svm, e.g);
    let seed = g.seed;
    let mut expected = entries
        .iter()
        .map(|(wallet, p)| {
            (
                h(&[b"GIVEAWAY_RANK_V2", &seed, wallet.as_ref()]),
                wallet.to_bytes(),
                *p,
            )
        })
        .collect::<Vec<_>>();
    expected.sort();
    for (ordinal, (_, wallet, participant)) in expected.iter().enumerate() {
        if ordinal % 32 == 0 {
            if ordinal > 0 {
                assert!(send(
                    &mut e.svm,
                    ix(
                        "next_wave",
                        &(),
                        vec![
                            M::new(e.g, false),
                            M::new(e.ranking, false),
                            M::new_readonly(e.host.pubkey(), true)
                        ]
                    ),
                    &e.host
                ));
            }
            for chunk in entries.chunks(4) {
                let keys = chunk.iter().map(|(_, key)| *key).collect::<Vec<_>>();
                assert!(coverage_chunk(&mut e, "process_wave", &keys));
            }
        }
        let ordinal = ordinal as u64;
        let c = pda(&[b"candidate", e.g.as_ref(), &ordinal.to_le_bytes()]);
        let s = pda(&[b"winner", e.g.as_ref(), &(ordinal as u32).to_le_bytes()]);
        assert!(send(
            &mut e.svm,
            ix(
                "open_candidate",
                &ordinal,
                vec![
                    M::new(e.g, false),
                    M::new(e.ranking, false),
                    M::new_readonly(*participant, false),
                    M::new(c, false),
                    M::new(s, false),
                    M::new(e.host.pubkey(), true),
                    M::new_readonly(system::id(), false)
                ]
            ),
            &e.host
        ));
        assert_eq!(account::<C>(&e.svm, c).wallet, *wallet);
        assert!(e.review(ordinal, true, false));
    }
    let before: G = account(&e.svm, e.g);
    assert_eq!(
        (before.approved_count, before.consumed_count, before.seed),
        (1000, 1000, seed)
    );
    assert!(e.lock());
    let vested: G = account(&e.svm, e.g);
    assert_eq!(
        (
            vested.status,
            vested.approved_count,
            vested.per_winner,
            vested.fee,
            vested.remainder
        ),
        (3, 1000, 95, 5000, 3)
    );
    let recipient = Pubkey::new_from_array(expected[999].1);
    let balance = e.svm.get_balance(&recipient).unwrap();
    e.clock(e.terms.absolute_deadline + 1);
    assert!(e.pay(999, recipient));
    assert!(e.pay(999, recipient));
    assert_eq!(e.svm.get_balance(&recipient).unwrap() - balance, 95);
    assert_eq!(account::<G>(&e.svm, e.g).paid_winners, 1);
    assert!(!e.refund());
    let metrics = RESOURCE_METRICS.lock().unwrap();
    let mut operations = serde_json::Map::new();
    for name in [
        "process_pool",
        "process_wave",
        "open_candidate",
        "review_candidate",
        "commit_outcome",
        "pay_winner",
    ] {
        let d: [u8; 8] = h(&[format!("global:{name}").as_bytes()])[..8]
            .try_into()
            .unwrap();
        if let Some(&(calls, total, maximum)) = metrics.get(&d) {
            operations.insert(name.into(),serde_json::json!({"calls":calls,"totalComputeUnits":total,"maximumComputeUnits":maximum}));
        }
    }
    let report = serde_json::json!({"qualification":"isolated actual SBF accepted-slot fixture; admission and human review time separate","participants":1000,"approvedWinners":1000,"candidateWaves":32,"wallSeconds":started.elapsed().as_secs_f64(),"hostLamportsSpent":host_before-e.svm.get_balance(&e.host.pubkey()).unwrap(),"perWinner":vested.per_winner,"fee":vested.fee,"creatorRemainder":vested.remainder,"operations":operations});
    eprintln!("MAX_APPROVED_RESOURCE_REPORT={report}");
    if let Some(path) = std::env::var_os("CHANCE_GIVEAWAY_BRAND_REVIEW_MAX_APPROVAL_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
