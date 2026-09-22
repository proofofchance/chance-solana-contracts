//! # ProofOfChance Giveaways Program
//!
//! A transparent, decentralized giveaway system built on Solana that uses
//! participant-provided entropy (proof-of-chance) for fair winner selection.
//!
//! ## Core Concepts
//!
//! ### Proof-of-Chance Model
//! - Participants provide a lucky phrase that gets hashed on-chain
//! - During the upload window, participants use a provider receipt or reveal directly on-chain
//! - Verified reveal digests are aggregated for transparent entropy generation
//! - Winners are selected deterministically using aggregated entropy
//!
//! ### Anti-Censorship Design
//! - Provider receipts may be relayed by any caller and bind the participant wallet
//! - Participants can bypass a refusing/unavailable provider with `AttestReveal`
//! - Provider must include all receipt-attested participants in reveal batches
//! - If any attested participant is missing, settlement is blocked
//!
//! ### Eligibility and Finalization
//! - Creator disqualification is restricted to the pre-settlement review window
//! - Only the configured service-provider authority may advance finalization
//! - Provider reveal publication waits for the deadline or every eligible attestation
//! - The final archival scan emits winners and locks the result atomically
//!
//! ## Account Architecture
//!
//! All accounts use Program Derived Addresses (PDAs) for security:
//! - **Config**: `["config"]` - Global system configuration
//! - **Giveaway**: `["giveaway", config_pubkey, giveaway_id_le_bytes]` - Individual giveaway state
//! - **Participant**: `["participant", giveaway_pubkey, wallet_pubkey]` - Participant data
//! - **Vault**: `["vault", giveaway_pubkey]` - Fund custody account
//! - **WinnersLedger**: `["winners_root_v2", giveaway_pubkey]` - Winner settlement data
//! - **FinalizationLedger**: `["finalization_root_v2", giveaway_pubkey]` - Fixed radix state
//!
//! ## Instruction Flow
//!
//! 1. **Initialize**: Set up global configuration (once per deployment)
//! 2. **CreateGiveaway**: Creator sets up giveaway with locked funds
//! 3. **Participate**: Participants submit proof text and proof-of-chance hash
//! 4. **AttestUploaded / AttestReveal**: Include a committed reveal through receipt or direct path
//! 5. **UploadReveals**: Provider uploads receipt-attested reveal batches
//! 6. **DisqualifyParticipant**: Creator can remove invalid entries before settlement
//! 7. **FinalizeWinners**: Run indexed aggregation, radix selection, and archival emission passes
//! 8. **SettlePayoutBatch**: Pay equal-value winners verified against the locked threshold
//!
//! ## Security Features
//!
//! - **PDA-based accounts**: All accounts use program-derived addresses
//! - **Locked funds**: Creator cannot withdraw; funds only go to winners
//! - **Transparent operations**: All actions emit events for audit trail
//! - **Anti-censorship**: Direct reveal and receipt relay paths remove provider liveness control
//! - **Deterministic selection**: Same entropy always produces same winners

#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;
pub mod utils;

use instructions::{
    attest_reveal::*, attest_uploaded::*, begin_upload_phase::*, close_participant::*,
    create_giveaway::*, disqualify_participant::*, extend_active_deadline::*, finalize_winners::*,
    initialize::*, lock_winners::*, participate::*, recompute_winners::*, registry::*,
    settle_giveaway::*, settle_no_eligible_giveaway::*, settle_payout_batch::*,
    update_service_charge::*, upload_reveals::*,
};

declare_id!("DUMRJ15A2ivmUNDK6EX7wfRQ1cYw4vw5ewSyT8xSJuRG");

#[program]
pub mod giveaways {
    use super::*;

    /// Initialize the giveaways system configuration
    ///
    /// Sets up the global config account with authority and default parameters.
    /// Can only be called once per deployment.
    ///
    /// Accounts expected:
    /// 0. `[writable]` Config account (PDA: ["config"])
    /// 1. `[signer]` Authority (becomes config.authority)
    /// 2. `[]` System program
    pub fn initialize(
        ctx: Context<Initialize>,
        service_fee_bps: u16,
        default_active_duration_secs: u32,
        default_upload_duration_secs: u32,
    ) -> Result<()> {
        instructions::initialize::process(
            ctx,
            service_fee_bps,
            default_active_duration_secs,
            default_upload_duration_secs,
        )
    }

    /// Create a new giveaway with locked funds
    ///
    /// Creator deposits the configured total payout amount. Service fee is
    /// taken from that amount during successful payout settlement.
    /// Giveaway parameters are stored on-chain for transparency.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account (PDA: ["giveaway", config, id])
    /// 2. `[writable]` Vault account (PDA: ["vault", giveaway])
    /// 3. `[signer, writable]` Creator (funds source)
    /// 4. `[]` Instructions sysvar
    pub fn create_giveaway(
        ctx: Context<CreateGiveaway>,
        giveaway_id: u64,
        total_payout_lamports: u64,
        number_of_winners: u32,
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_duration_secs: u32,
    ) -> Result<()> {
        instructions::create_giveaway::process(
            ctx,
            giveaway_id,
            total_payout_lamports,
            number_of_winners,
            active_start_unix,
            active_deadline_unix,
            upload_duration_secs,
        )
    }

    /// One-time canonical registry binding, before any instance is created.
    pub fn bind_registry(ctx: Context<BindRegistry>) -> Result<()> {
        instructions::registry::bind(ctx)
    }

    /// Durable creator-supplied nonce prevents duplicate creation across releases.
    #[allow(clippy::too_many_arguments)]
    pub fn create_giveaway_with_nonce(
        ctx: Context<CreateGiveaway>,
        giveaway_id: u64,
        total_payout_lamports: u64,
        number_of_winners: u32,
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_duration_secs: u32,
        creator_nonce: u64,
    ) -> Result<()> {
        instructions::create_giveaway::process_with_nonce(
            ctx,
            giveaway_id,
            total_payout_lamports,
            number_of_winners,
            active_start_unix,
            active_deadline_unix,
            upload_duration_secs,
            creator_nonce,
        )
    }

    /// Participate in a giveaway
    ///
    /// Submit proof text and proof-of-chance commitment.
    /// Can be called multiple times to update entry before deadline.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Participant account (PDA: ["participant", giveaway, wallet])
    /// 3. `[signer]` Participant wallet
    /// 4. `[]` System program
    pub fn participate(
        ctx: Context<Participate>,
        commitment_hash: [u8; 32],
        proof_text: String,
    ) -> Result<()> {
        instructions::participate::process(ctx, commitment_hash, proof_text)
    }

    /// Attest that proof-of-chance has been uploaded to provider
    ///
    /// Anti-censorship mechanism - provider must include all attested participants.
    /// Can only be called during upload/attestation window.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[]` Giveaway account
    /// 2. `[writable]` Participant account
    /// 3. `[signer]` Participant wallet
    pub fn attest_uploaded(ctx: Context<AttestUploaded>) -> Result<()> {
        instructions::attest_uploaded::process(ctx)
    }

    /// Verify and include the participant's committed reveal directly on-chain.
    /// This path does not require a provider receipt.
    pub fn attest_reveal(
        ctx: Context<AttestReveal>,
        lucky_words: String,
        salt: Vec<u8>,
    ) -> Result<()> {
        instructions::attest_reveal::process(ctx, lucky_words, salt)
    }

    /// Upload batch of participant reveals
    ///
    /// Provider uploads plaintext proof-of-chance data for entropy generation.
    /// Publication waits for the deadline or every eligible participant to
    /// attest. All attested participants must be included before settlement.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Authority
    /// 3..N. `[writable]` Participant accounts
    pub fn upload_reveals(ctx: Context<UploadReveals>, reveals: Vec<RevealData>) -> Result<()> {
        instructions::upload_reveals::process(ctx, reveals)
    }

    /// Finalize winners and store merkle commitment
    ///
    /// Provider-authorized instruction that processes a chunk of participants.
    /// Once every participant is processed, stores the winner commitment
    /// on-chain for batch settlement verification.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[writable]` WinnersLedger account (PDA: ["winners_ledger", giveaway])
    /// 4. `[writable]` FinalizationLedger account (PDA: ["finalization_root_v2", giveaway])
    /// 5. `[signer]` Authority
    /// 6. `[]` System program
    /// 7..N. `[writable]` Reveal-included participant accounts for this chunk
    pub fn finalize_winners(ctx: Context<FinalizeWinners>) -> Result<()> {
        instructions::finalize_winners::process(ctx)
    }

    /// Disqualify a participant entry
    ///
    /// Only giveaway creator can disqualify participants.
    /// All disqualifications are logged for transparency.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Participant account
    /// 3. `[signer]` Creator
    pub fn disqualify_participant(
        ctx: Context<DisqualifyParticipant>,
        reason_code: u8,
    ) -> Result<()> {
        instructions::disqualify_participant::process(ctx, reason_code)
    }

    /// Recompute winners after disqualifications
    ///
    /// Disabled in the scalable finalization protocol. Disqualifications happen
    /// before upload; `finalize_winners` processes the resulting eligible set in chunks.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` WinnersLedger account
    /// 3. `[signer]` Creator or Authority
    /// 4..N. `[]` All participant accounts
    pub fn recompute_winners(ctx: Context<RecomputeWinners>) -> Result<()> {
        instructions::recompute_winners::process(ctx)
    }

    /// Lock winners to prevent further changes
    ///
    /// Irreversible action that enables settlement.
    /// No more disqualifications or recomputes allowed after this.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` WinnersLedger account
    /// 3. `[signer]` Creator
    pub fn lock_winners(ctx: Context<LockWinners>) -> Result<()> {
        instructions::lock_winners::process(ctx)
    }

    /// Refund a giveaway that never reached winner finalization
    ///
    /// Handles no participants, no attesters, and expired omitted-reveal remediation
    /// without initializing a winners ledger.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[signer, writable]` Creator
    /// 4. `[signer]` Authority (can be same as creator)
    pub fn settle_giveaway(ctx: Context<SettleGiveaway>) -> Result<()> {
        instructions::settle_giveaway::process(ctx)
    }

    /// Refund a finalized giveaway with zero eligible winners
    ///
    /// Requires an existing winners ledger proving finalization completed with
    /// `winners_count == 0`. This instruction never initializes the ledger.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[]` WinnersLedger account (PDA: ["winners_ledger", giveaway])
    /// 4. `[signer, writable]` Creator
    /// 5. `[signer]` Authority (can be same as creator)
    pub fn settle_no_eligible_giveaway(ctx: Context<SettleNoEligibleGiveaway>) -> Result<()> {
        instructions::settle_no_eligible_giveaway::process(ctx)
    }

    /// Process a batch of winner payouts
    ///
    /// Verifies merkle proofs and transfers funds directly to winners.
    /// Can be called multiple times to process all winners in batches.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[writable]` WinnersLedger account
    /// 4. `[signer, writable]` Authority
    /// 5. `[writable]` Creator refund account
    /// 6..N. `[writable]` Winner wallet accounts
    pub fn settle_payout_batch<'info>(
        ctx: Context<'_, '_, '_, 'info, SettlePayoutBatch<'info>>,
        batch_index: u32,
        winners: Vec<WinnerProof>,
    ) -> Result<()> {
        instructions::settle_payout_batch::process(ctx, batch_index, winners)
    }

    /// Extend active deadline (only before current deadline)
    ///
    /// Allows creator to extend participation window.
    /// Cannot shorten or extend after deadline has passed.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Creator
    pub fn extend_active_deadline(
        ctx: Context<ExtendActiveDeadline>,
        new_deadline_unix: i64,
    ) -> Result<()> {
        instructions::extend_active_deadline::process(ctx, new_deadline_unix)
    }

    /// Begin upload phase immediately
    ///
    /// Authority-only instruction to start upload/attestation phase now.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Authority
    pub fn begin_upload_phase(ctx: Context<BeginUploadPhase>) -> Result<()> {
        instructions::begin_upload_phase::process(ctx)
    }

    /// Update service charge rate (admin only)
    ///
    /// Updates the global service fee rate.
    /// Only affects new giveaways created after this change.
    ///
    /// Accounts expected:
    /// 0. `[writable]` Config account
    /// 1. `[signer]` Authority
    pub fn update_service_charge(
        ctx: Context<UpdateServiceCharge>,
        new_service_fee_bps: u16,
    ) -> Result<()> {
        instructions::update_service_charge::process(ctx, new_service_fee_bps)
    }

    /// Close a participant account after giveaway settlement and reclaim rent
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[]` Giveaway account
    /// 2. `[writable]` Participant account
    /// 3. `[signer, writable]` Participant wallet
    pub fn close_participant(ctx: Context<CloseParticipant>) -> Result<()> {
        instructions::close_participant::process(ctx)
    }
}
