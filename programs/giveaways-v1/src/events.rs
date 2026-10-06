//! # Event Definitions
//!
//! Defines all events emitted by the giveaways program for transparency
//! and integration with indexing systems.

use anchor_lang::solana_program::msg;
use serde::{Deserialize, Serialize};

/// Version identifier for event schema
pub const EVENT_VERSION: &str = "1.0.0";

/// Event wrapper for structured emission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiveawayEventWrapper {
    pub version: String,
    pub program: String,
    pub event: GiveawayEvent,
}

/// All possible events emitted by the giveaways program
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data")]
pub enum GiveawayEvent {
    GiveawayCreated {
        giveaway_id: u64,
        giveaway: String,
        creator: String,
        config: String,
        vault: String,
        total_payout_lamports: u64,
        number_of_winners: u32,
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_start_unix: i64,
        upload_deadline_unix: i64,
        service_fee_bps: u16,
        timestamp: i64,
    },
    GiveawayUpdated {
        giveaway_id: u64,
        giveaway: String,
        active_deadline_unix: Option<i64>,
        upload_deadline_unix: Option<i64>,
        timestamp: i64,
    },
    ParticipationSubmitted {
        giveaway_id: u64,
        giveaway: String,
        participant: String,
        participant_account: String,
        commitment_hash: String,
        proof_text: String,
        is_update: bool,
        participant_index: u64,
        timestamp: i64,
    },
    AttestationSubmitted {
        giveaway_id: u64,
        giveaway: String,
        participant: String,
        participant_account: String,
        timestamp: i64,
    },
    RevealsUploaded {
        giveaway_id: u64,
        giveaway: String,
        authority: String,
        batch_size: u32,
        total_reveals_uploaded: u64,
        total_attested: u64,
        aggregate_hash: String,
        uploads_complete: bool,
        timestamp: i64,
    },
    RevealRemediationBegan {
        giveaway_id: u64,
        giveaway: String,
        included_reveals_count: u64,
        attested_count: u64,
        remediation_start_unix: i64,
        remediation_deadline_unix: i64,
        timestamp: i64,
    },
    RevealRemediationCompleted {
        giveaway_id: u64,
        giveaway: String,
        included_reveals_count: u64,
        attested_count: u64,
        timestamp: i64,
    },
    UploadPhaseBegan {
        giveaway_id: u64,
        giveaway: String,
        upload_start_unix: i64,
        upload_deadline_unix: i64,
        timestamp: i64,
    },
    WinnersComputed {
        giveaway_id: u64,
        giveaway: String,
        winners_ledger: String,
        merkle_root: String,
        seed: String,
        rule_version: String,
        total_eligible: u64,
        winners_count: u32,
        total_payout_lamports: u64,
        per_winner_lamports: u64,
        recompute_version: u32,
        winners: Vec<String>,
        timestamp: i64,
    },
    /// Emitted during the canonical post-threshold scan so immutable replay can
    /// reconstruct every winner without reading participant accounts.
    WinnerSelected {
        giveaway_id: u64,
        giveaway: String,
        participant: String,
        participant_account: String,
        candidate_key: String,
        recompute_version: u32,
        emission_index: u32,
        timestamp: i64,
    },
    FinalizationChunkProcessed {
        giveaway_id: u64,
        giveaway: String,
        finalization_ledger: String,
        recompute_version: u32,
        batch_size: u32,
        processed_count: u64,
        required_count: u64,
        eligible_count: u64,
        candidate_count: u32,
        completed: bool,
        timestamp: i64,
    },
    ParticipantDisqualified {
        giveaway_id: u64,
        giveaway: String,
        participant: String,
        participant_account: String,
        reason_code: u8,
        disqualified_by: String,
        timestamp: i64,
    },
    WinnersLocked {
        giveaway_id: u64,
        giveaway: String,
        winners_ledger: String,
        final_recompute_version: u32,
        final_winners_count: u32,
        timestamp: i64,
    },
    WinnerPaid {
        giveaway_id: u64,
        giveaway: String,
        winner: String,
        amount_lamports: u64,
        batch_index: u32,
        winner_index: u32,
        timestamp: i64,
    },
    ServiceFeePaid {
        giveaway_id: u64,
        giveaway: String,
        service_fee_lamports: u64,
        authority: String,
        timestamp: i64,
    },
    NoWinners {
        giveaway_id: u64,
        giveaway: String,
        reason: String,
        total_participants: u64,
        total_attested: u64,
        timestamp: i64,
    },
    CreatorRefunded {
        giveaway_id: u64,
        giveaway: String,
        creator: String,
        amount_lamports: u64,
        timestamp: i64,
    },
    GiveawaySettled {
        giveaway_id: u64,
        giveaway: String,
        total_winners: u32,
        total_paid_lamports: u64,
        timestamp: i64,
    },
    ParticipantClosed {
        giveaway_id: u64,
        giveaway: String,
        participant: String,
        participant_account: String,
        rent_reclaimed: u64,
        timestamp: i64,
    },
}

impl GiveawayEvent {
    /// Emit this event as a structured log message
    pub fn emit(&self) {
        let event_wrapper = GiveawayEventWrapper {
            version: EVENT_VERSION.to_string(),
            program: "giveaways".to_string(),
            event: self.clone(),
        };

        // Emit as JSON log that indexers can parse
        if let Ok(json) = serde_json::to_string(&event_wrapper) {
            msg!("GIVEAWAY_EVENT: {}", json);
        }
    }
}
