//! # Instructions Module
//!
//! This module contains all the instruction definitions and their processing logic
//! for the giveaways program. Each instruction is implemented as a separate
//! module for better organization.

pub mod attest_reveal;
pub mod attest_uploaded;
pub mod begin_upload_phase;
pub mod close_participant;
pub mod create_giveaway;
pub mod disqualify_participant;
pub mod extend_active_deadline;
pub mod finalize_winners;
pub mod initialize;
pub mod lock_winners;
pub mod participate;
pub mod recompute_winners;
pub mod settle_giveaway;
pub mod settle_no_eligible_giveaway;
pub mod settle_payout_batch;
pub mod update_service_charge;
pub mod upload_reveals;

// Re-export only the instruction account context structs and input types
pub use attest_reveal::AttestReveal;
pub use attest_uploaded::AttestUploaded;
pub use begin_upload_phase::BeginUploadPhase;
pub use close_participant::CloseParticipant;
pub use create_giveaway::CreateGiveaway;
pub use disqualify_participant::DisqualifyParticipant;
pub use extend_active_deadline::ExtendActiveDeadline;
pub use finalize_winners::FinalizeWinners;
pub use initialize::Initialize;
pub use lock_winners::LockWinners;
pub use participate::Participate;
pub use recompute_winners::RecomputeWinners;
pub use settle_giveaway::SettleGiveaway;
pub use settle_no_eligible_giveaway::SettleNoEligibleGiveaway;
pub use settle_payout_batch::{SettlePayoutBatch, WinnerProof};
pub use update_service_charge::UpdateServiceCharge;
pub use upload_reveals::{RevealData, UploadReveals};

pub mod registry;
pub use registry::BindRegistry;
