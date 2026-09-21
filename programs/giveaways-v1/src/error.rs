//! # Error Definitions
//!
//! Defines all custom error types for the giveaways program.
//! Each error includes a descriptive message for debugging.

use anchor_lang::prelude::*;

#[error_code]
pub enum GiveawayError {
    #[msg("Unauthorized: Only the authority can perform this action")]
    Unauthorized,

    #[msg("Invalid instruction: Operation not allowed in current state")]
    InvalidInstruction,

    #[msg("Giveaway already settled: Cannot modify settled giveaway")]
    GiveawayAlreadySettled,

    #[msg("Invalid timing: Start time must be before deadline")]
    InvalidTiming,

    #[msg("Active window closed: Cannot participate after deadline")]
    ActiveWindowClosed,

    #[msg("Active window not started: Cannot participate before start time")]
    ActiveWindowNotStarted,

    #[msg("Upload window closed: Cannot attest after upload deadline")]
    UploadWindowClosed,

    #[msg("Upload window not started: Cannot attest before upload start")]
    UploadWindowNotStarted,

    #[msg("Invalid attestation: Provider receipt signature is invalid or missing")]
    InvalidAttestation,

    #[msg("Settlement not ready: Upload window must end or all eligible participants must attest")]
    SettlementNotReady,

    #[msg("Text too long: Exceeds maximum allowed length")]
    TextTooLong,

    #[msg("Invalid service fee: Must be between 0 and 9999 basis points")]
    InvalidServiceFee,

    #[msg("Invalid winner count: Must be between 1 and 1000")]
    InvalidWinnerCount,

    #[msg("Participant limit reached for this giveaway")]
    TooManyParticipants,

    #[msg("Invalid duration: Duration outside allowed range")]
    InvalidDuration,

    #[msg("Insufficient funds: Not enough lamports for payout and fees")]
    InsufficientFunds,

    #[msg("Invalid reveal: Plaintext does not match commitment hash")]
    InvalidReveal,

    #[msg("Participant not found: No participation record exists")]
    ParticipantNotFound,

    #[msg("Already attested: Participant has already attested upload")]
    AlreadyAttested,

    #[msg("Not attested: Participant must attest before settlement")]
    NotAttested,

    #[msg("Already disqualified: Participant is already disqualified")]
    AlreadyDisqualified,

    #[msg("Winners not computed: Must finalize winners before settlement")]
    WinnersNotComputed,

    #[msg("Winners locked: Cannot modify winners after locking")]
    WinnersLocked,

    #[msg("Winners not locked: Winner set must be locked before payout settlement")]
    WinnersNotLocked,

    #[msg("Invalid merkle proof: Proof verification failed")]
    InvalidMerkleProof,

    #[msg("Already paid: Winner has already been paid")]
    AlreadyPaid,

    #[msg("Invalid batch: Batch index out of range")]
    InvalidBatch,

    #[msg("Deadline passed: Cannot extend deadline after it has passed")]
    DeadlinePassed,

    #[msg("Cannot shorten: New deadline must be later than current")]
    CannotShorten,

    #[msg("Math overflow: Arithmetic operation overflowed")]
    MathOverflow,

    #[msg("Division by zero: Cannot divide by zero")]
    DivisionByZero,

    #[msg("Invalid account: Account does not match expected PDA")]
    InvalidAccount,

    #[msg("Account not writable: Account must be writable for this operation")]
    AccountNotWritable,

    #[msg("Missing signer: Required signature not provided")]
    MissingSigner,

    #[msg("Invalid program: Account not owned by expected program")]
    InvalidProgram,

    #[msg("Account already initialized: Cannot initialize twice")]
    AccountAlreadyInitialized,

    #[msg("Account not initialized: Account must be initialized first")]
    AccountNotInitialized,

    #[msg("Reveal already included: Participant reveal already processed")]
    RevealAlreadyIncluded,

    #[msg("Missing attested participants: Some attested participants not included in reveals")]
    MissingAttestedParticipants,

    #[msg("No eligible participants: Cannot compute winners without eligible participants")]
    NoEligibleParticipants,

    #[msg("Giveaway not found: Giveaway account does not exist")]
    GiveawayNotFound,

    #[msg("Invalid giveaway ID: Giveaway ID must be unique")]
    InvalidGiveawayId,

    #[msg("Creator mismatch: Only giveaway creator can perform this action")]
    CreatorMismatch,

    #[msg("Invalid reason code: Disqualification reason code is invalid")]
    InvalidReasonCode,

    #[msg("Early upload disabled: Build must enable the allow-early-upload feature")]
    EarlyUploadDisabled,

    #[msg("Reveal publication not ready: Upload window must end or all eligible participants must attest")]
    RevealPublicationNotReady,
}
