#![allow(clippy::result_large_err)]

use std::{
    env,
    path::{Path, PathBuf},
};

use litesvm::{
    types::{FailedTransactionMetadata, TransactionResult},
    LiteSVM,
};
use solana_account::Account;
use solana_instruction::{error::InstructionError, Instruction};
use solana_keypair::Keypair;
use solana_program::{clock::Clock, pubkey::Pubkey};
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

const DEFAULT_AIRDROP_LAMPORTS: u64 = 10_000_000_000;
#[allow(dead_code)]
const APPROX_SECONDS_PER_SLOT_DIVISOR: u64 = 2;

pub struct TestContext {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub registry: Pubkey,
    pub daily_program: Pubkey,
}

#[allow(dead_code)]
impl TestContext {
    pub fn new(program_id: Pubkey, extra_funded_accounts: &[&Keypair]) -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(program_id, program_binary_path("daily_lottery"))
            .expect("failed to load daily_lottery SBF artifact");

        let mut clock = svm.get_sysvar::<Clock>();
        clock.slot = 1;
        clock.unix_timestamp = 1;
        clock.epoch_start_timestamp = 1;
        svm.set_sysvar::<Clock>(&clock);

        let payer = Keypair::new();
        let mut funded_accounts = Vec::with_capacity(extra_funded_accounts.len() + 1);
        funded_accounts.push(&payer);
        funded_accounts.extend_from_slice(extra_funded_accounts);

        for keypair in funded_accounts {
            svm.airdrop(&keypair.pubkey(), DEFAULT_AIRDROP_LAMPORTS)
                .expect("airdrop should succeed");
        }

        let mut context = Self {
            svm,
            payer,
            registry: Pubkey::new_unique(),
            daily_program: program_id,
        };
        context.setup_registry(program_id);
        context
    }

    pub fn get_account(&self, address: Pubkey) -> Option<Account> {
        self.svm.get_account(&address)
    }

    pub fn set_account(&mut self, address: Pubkey, account: Account) {
        self.svm
            .set_account(address, account)
            .expect("set_account should succeed");
    }

    pub fn send_tx(
        &mut self,
        instructions: Vec<Instruction>,
        signers: &[&Keypair],
    ) -> TransactionResult {
        let instructions = self.prepare_creation(instructions);
        let mut tx = Transaction::new_with_payer(&instructions, Some(&self.payer.pubkey()));
        let mut all_signers = vec![&self.payer];
        all_signers.extend_from_slice(signers);
        tx.sign(&all_signers, self.svm.latest_blockhash());
        self.svm.send_transaction(tx)
    }

    pub fn warp_to_slot(&mut self, slot: u64) {
        let previous_clock = self.svm.get_sysvar::<Clock>();
        self.svm.warp_to_slot(slot);
        let mut clock = self.svm.get_sysvar::<Clock>();
        let slots_advanced = slot.saturating_sub(previous_clock.slot);
        clock.slot = slot;
        clock.unix_timestamp = previous_clock
            .unix_timestamp
            .saturating_add((slots_advanced / APPROX_SECONDS_PER_SLOT_DIVISOR) as i64);
        self.svm.set_sysvar::<Clock>(&clock);
    }

    pub fn get_clock(&self) -> Clock {
        self.svm.get_sysvar::<Clock>()
    }

    pub fn set_clock(&mut self, clock: &Clock) {
        self.svm.set_sysvar::<Clock>(clock);
    }
}

#[allow(dead_code)]
pub fn assert_custom_error(err: FailedTransactionMetadata, code: u32) {
    match err.err {
        TransactionError::InstructionError(_, InstructionError::Custom(actual)) => {
            assert_eq!(actual, code)
        }
        other => panic!("expected custom program error, got {other:?}"),
    }
}

fn program_binary_path(program_name: &str) -> PathBuf {
    let file_name = format!("{program_name}.so");
    for base in candidate_program_dirs() {
        let path = base.join(&file_name);
        if path.exists() {
            return path;
        }
    }

    panic!(
        "missing SBF artifact `{file_name}`; run `cargo test-sbf -p daily_lottery` so LiteSVM can load the compiled program"
    );
}

fn candidate_program_dirs() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dirs = Vec::new();

    for var in ["BPF_OUT_DIR", "SBF_OUT_DIR"] {
        if let Some(value) = env::var_os(var) {
            dirs.push(PathBuf::from(value));
        }
    }

    dirs.push(manifest_dir.join("../../target/sbpf-solana-solana/release"));
    dirs.push(manifest_dir.join("../target/sbpf-solana-solana/release"));
    dirs.push(manifest_dir.join("target/sbpf-solana-solana/release"));
    dirs.push(manifest_dir.join("target/deploy"));
    dirs.push(manifest_dir.join("../../target/deploy"));
    dirs.push(manifest_dir.join("../target/deploy"));
    dirs.into_iter().map(normalize_path).collect::<Vec<_>>()
}

fn normalize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

/// Build an actual immutable registry in each isolated SVM fixture. Tests exercise
/// real registry CPIs; no test feature disables the production creation gate.
impl TestContext {
    fn setup_registry(&mut self, program_id: Pubkey) {
        use chance_registry_wire as w;
        use solana_instruction::AccountMeta as M;
        let reg = self.registry;
        self.svm
            .add_program_from_file(reg, program_binary_path("chance_registry"))
            .unwrap();
        let cfg =
            Pubkey::find_program_address(&[b"registry", self.payer.pubkey().as_ref()], &reg).0;
        let release =
            Pubkey::find_program_address(&[b"release", cfg.as_ref(), program_id.as_ref()], &reg).0;
        let system = solana_system_interface::program::id();
        let instructions = [
            Instruction {
                program_id: reg,
                accounts: vec![
                    M::new(cfg, false),
                    M::new(self.payer.pubkey(), true),
                    M::new_readonly(system, false),
                ],
                data: w::to_vec(&w::Instruction::Initialize {
                    guardian: self.payer.pubkey().to_bytes(),
                    activation_delay: 1,
                })
                .unwrap(),
            },
            Instruction {
                program_id: reg,
                accounts: vec![
                    M::new(cfg, false),
                    M::new(release, false),
                    M::new(self.payer.pubkey(), true),
                    M::new_readonly(system, false),
                    M::new_readonly(program_id, false),
                ],
                data: w::to_vec(&w::Instruction::Register {
                    domain: w::DAILY,
                    series: [1; 32],
                    source_hash: [2; 32],
                    executable_hash: {
                        let bytes = std::fs::read(program_binary_path("daily_lottery")).unwrap();
                        let end = bytes.iter().rposition(|b| *b != 0).unwrap() + 1;
                        solana_sha256_hasher::hash(&bytes[..end]).to_bytes()
                    },
                })
                .unwrap(),
            },
        ];
        self.send_tx(instructions.to_vec(), &[]).unwrap();
        let mut clock = self.get_clock();
        clock.unix_timestamp += 1;
        self.set_clock(&clock);
        let loader = solana_program::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
        let record =
            Pubkey::find_program_address(&[b"record", cfg.as_ref(), &1u64.to_le_bytes()], &reg).0;
        let activation = Instruction {
            program_id: reg,
            accounts: vec![
                M::new(cfg, false),
                M::new(release, false),
                M::new(self.payer.pubkey(), true),
                M::new_readonly(system, false),
                M::new(record, false),
                M::new_readonly(program_id, false),
                M::new_readonly(
                    Pubkey::find_program_address(&[program_id.as_ref()], &loader).0,
                    false,
                ),
                M::new_readonly(reg, false),
                M::new_readonly(
                    Pubkey::find_program_address(&[reg.as_ref()], &loader).0,
                    false,
                ),
            ],
            data: w::to_vec(&w::Instruction::Activate {
                reason: "isolated runtime test".into(),
            })
            .unwrap(),
        };
        let budget = Instruction { program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
            accounts: vec![], data: [vec![2], 1_400_000u32.to_le_bytes().to_vec()].concat() };
        self.send_tx(vec![budget, activation], &[]).unwrap();
    }

    fn prepare_creation(&self, mut instructions: Vec<Instruction>) -> Vec<Instruction> {
        use chance_registry_wire as w;
        use solana_instruction::AccountMeta as M;
        let mut result = Vec::new();
        for mut ix in instructions.drain(..) {
            if ix.program_id != self.daily_program
                || ix.data.first() != Some(&2)
                || ix.accounts.len() != 5
            {
                result.push(ix);
                continue;
            }
            let reg = self.registry;
            let cfg =
                Pubkey::find_program_address(&[b"registry", self.payer.pubkey().as_ref()], &reg).0;
            let release = Pubkey::find_program_address(
                &[b"release", cfg.as_ref(), self.daily_program.as_ref()],
                &reg,
            )
            .0;
            let config_account = self.svm.get_account(&ix.accounts[0].pubkey).unwrap();
            let config: daily_lottery::Config =
                borsh::from_slice(&config_account.data[8..]).unwrap();
            if config.registry_program == Pubkey::default() {
                result.push(Instruction {
                    program_id: self.daily_program,
                    accounts: vec![
                        ix.accounts[0].clone(),
                        ix.accounts[3].clone(),
                        M::new_readonly(reg, false),
                        M::new_readonly(cfg, false),
                        M::new_readonly(release, false),
                    ],
                    data: borsh::to_vec(&daily_lottery::Instruction::BindRegistry).unwrap(),
                });
            }
            let state: w::Config =
                w::from_slice(&self.svm.get_account(&cfg).unwrap().data).unwrap();
            let index = (state.instance_count + 1).to_le_bytes();
            let day = (self.get_clock().unix_timestamp as u64 / 86_400).to_le_bytes();
            let key =
                solana_sha256_hasher::hashv(&[b"chance-daily-key-v1", &[1; 32], &day]).to_bytes();
            ix.accounts.extend([
                M::new_readonly(reg, false),
                M::new(cfg, false),
                M::new_readonly(release, false),
                M::new_readonly(
                    Pubkey::find_program_address(
                        &[b"chance-release", cfg.as_ref()],
                        &self.daily_program,
                    )
                    .0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(&[b"instance", cfg.as_ref(), &index], &reg).0,
                    false,
                ),
                M::new(
                    Pubkey::find_program_address(
                        &[b"business", cfg.as_ref(), &[w::DAILY], &key],
                        &reg,
                    )
                    .0,
                    false,
                ),
            ]);
            result.push(ix);
        }
        result
    }
}
