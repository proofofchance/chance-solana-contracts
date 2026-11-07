// V1 brand-review schema: public tasks/evidence and final brand judgment.
// This source change grants no deployment or activation approval.
use anchor_lang::prelude::*;
#[path = "brand_review/contexts.rs"]
pub mod contexts;
#[path = "brand_review/crypto.rs"]
pub mod crypto;
#[path = "brand_review/handlers.rs"]
pub mod handlers;
#[path = "brand_review/registry.rs"]
pub mod registry;
#[path = "brand_review/state.rs"]
pub mod state;
use contexts::*;
use registry::*;
use state::*;

#[cfg(not(feature = "staging-afresh-v1"))]
declare_id!("EahQmXc3rwhY3CH1g3ZgUx8L4vHTNmzpK1xtiQ1RAxq6");
#[cfg(feature = "staging-afresh-v1")]
include!(concat!(env!("OUT_DIR"), "/staging_afresh.rs"));
#[cfg(not(feature = "staging-afresh-v1"))]
pub const INITIALIZATION_AUTHORITY: Pubkey =
    solana_program::pubkey!("Eechuq54TEzmBHXX7Ltbmpu6K5Vf3qgdsRdYmn41vJ9o");

#[program]
pub mod giveaways {
    use super::*;
    pub fn initialize(
        ctx: Context<Initialize>,
        host: Pubkey,
        fee_bps: u16,
        network_domain: [u8; 32],
    ) -> Result<()> {
        handlers::initialize(ctx, host, fee_bps, network_domain)
    }
    pub fn bind_registry(ctx: Context<BindRegistry>) -> Result<()> {
        registry::bind(ctx)
    }
    pub fn begin_rulebook(ctx: Context<BeginRulebook>, nonce: u64, task_count: u16) -> Result<()> {
        handlers::begin_rulebook(ctx, nonce, task_count)
    }
    pub fn append_task(ctx: Context<AppendTask>, input: TaskInput) -> Result<()> {
        handlers::append_task(ctx, input)
    }
    pub fn seal_rulebook(ctx: Context<RulebookOwner>) -> Result<()> {
        handlers::seal_rulebook(ctx)
    }
    pub fn close_unbound_rulebook(ctx: Context<CloseRulebook>) -> Result<()> {
        handlers::close_rulebook(ctx)
    }
    pub fn create_checked(
        ctx: Context<Create>,
        id: u64,
        terms: Terms,
        expected_creation_hash: [u8; 32],
    ) -> Result<()> {
        handlers::create(ctx, id, terms, expected_creation_hash)
    }
    pub fn begin_entry(ctx: Context<BeginEntry>, commitment: [u8; 32]) -> Result<()> {
        handlers::begin_entry(ctx, commitment)
    }
    pub fn revise_commitment(ctx: Context<EntryOwner>, commitment: [u8; 32]) -> Result<()> {
        handlers::revise_commitment(ctx, commitment)
    }
    pub fn publish_evidence(
        ctx: Context<PublishEvidence>,
        task_id: u16,
        version: u8,
        text: String,
    ) -> Result<()> {
        handlers::publish_evidence(ctx, task_id, version, text)
    }
    pub fn begin_entry_seal(ctx: Context<BeginSeal>) -> Result<()> {
        handlers::begin_seal(ctx)
    }
    pub fn process_entry_seal(ctx: Context<ProcessSeal>) -> Result<()> {
        handlers::process_seal(ctx)
    }
    pub fn finish_entry_seal(ctx: Context<ProcessSeal>) -> Result<()> {
        handlers::finish_seal(ctx)
    }
    pub fn attest_receipt(ctx: Context<Receipt>) -> Result<()> {
        handlers::attest_receipt(ctx)
    }
    pub fn reveal(ctx: Context<Reveal>, lucky_phrase: String, salt: Vec<u8>) -> Result<()> {
        handlers::reveal(ctx, lucky_phrase, salt)
    }
    pub fn begin_pool(ctx: Context<BeginPool>) -> Result<()> {
        handlers::begin_pool(ctx)
    }
    pub fn process_pool(ctx: Context<RankChunk>) -> Result<()> {
        handlers::process_pool(ctx)
    }
    pub fn process_wave(ctx: Context<RankChunk>) -> Result<()> {
        handlers::process_wave(ctx)
    }
    pub fn next_wave(ctx: Context<RankChunk>) -> Result<()> {
        handlers::next_wave(ctx)
    }
    pub fn open_candidate(ctx: Context<OpenCandidate>, ordinal: u64) -> Result<()> {
        handlers::open_candidate(ctx, ordinal)
    }
    pub fn review_candidate(
        ctx: Context<ReviewCandidate>,
        approve: bool,
        reason: u8,
        note: String,
        task_mask: u32,
    ) -> Result<()> {
        handlers::review_candidate(ctx, approve, reason, note, task_mask)
    }
    pub fn commit_outcome(ctx: Context<Finalize>) -> Result<()> {
        handlers::commit_outcome(ctx)
    }
    pub fn finalize_refund(ctx: Context<Refund>) -> Result<()> {
        handlers::finalize_refund(ctx)
    }
    pub fn pay_winner(ctx: Context<PayWinner>) -> Result<()> {
        handlers::pay_winner(ctx)
    }
    pub fn pay_beneficiaries(ctx: Context<PayBeneficiaries>) -> Result<()> {
        handlers::pay_beneficiaries(ctx)
    }
    pub fn withdraw_surplus(ctx: Context<Refund>) -> Result<()> {
        handlers::withdraw_surplus(ctx)
    }
}
