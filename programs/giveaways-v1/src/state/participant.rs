//! # Participant Account State
//!
//! The Participant account stores individual participant data including
//! proof text, commitment hash, and attestation status.

use crate::constants::*;
use anchor_lang::prelude::*;

/// Individual participant state
///
/// Each participant has one account per giveaway containing their
/// proof text, commitment hash, and participation status.
///
/// ## PDA Seeds
/// `["participant", giveaway_pubkey, wallet_pubkey]`
#[account]
pub struct Participant {
    /// Giveaway this participant is enrolled in
    pub giveaway: Pubkey,

    /// Participant wallet address
    pub wallet: Pubkey,

    /// Proof-of-chance commitment hash (SHA256 of lucky_words || 0x1f || salt)
    pub commitment_hash: [u8; 32],

    /// Proof text submitted by participant (social links, handles, etc.)
    pub proof_text: String,

    /// Whether participant has attested to uploading reveal
    pub attested_uploaded: bool,

    /// When attestation was submitted (0 if not attested)
    pub attested_at_unix: i64,

    /// Whether reveal has been included in provider upload
    pub reveal_included: bool,

    /// Whether participant has been disqualified
    pub disqualified: bool,

    /// Disqualification reason code (0 if not disqualified)
    pub disqualification_reason: u8,

    /// When participant was disqualified (0 if not disqualified)
    pub disqualified_at_unix: i64,

    /// When this participant entry was created
    pub created_at_unix: i64,

    /// When this participant entry was last updated
    pub last_updated_unix: i64,

    /// Reserved space for future fields
    pub reserved: [u8; 64],
}

impl Participant {
    const FINALIZATION_VERSION_OFFSET: usize = 0;
    const FINALIZATION_VERSION_LEN: usize = 4;
    const SELECTION_VERSION_OFFSET: usize = 4;
    const SELECTION_PASS_OFFSET: usize = 8;
    const PAYOUT_VERSION_OFFSET: usize = 10;
    const REVEAL_DIGEST_OFFSET: usize = 16;
    const REVEAL_DIGEST_LEN: usize = 32;
    const PARTICIPANT_INDEX_OFFSET: usize = 48;
    const PARTICIPANT_INDEX_LEN: usize = 8;

    /// Maximum size of Participant account in bytes
    pub const MAX_SIZE: usize = 8 + // discriminator
        32 +  // giveaway
        32 +  // wallet
        32 +  // commitment_hash
        4 + MAX_PROOF_TEXT_LEN + // proof_text
        1 +   // attested_uploaded
        8 +   // attested_at_unix
        1 +   // reveal_included
        1 +   // disqualified
        1 +   // disqualification_reason
        8 +   // disqualified_at_unix
        8 +   // created_at_unix
        8 +   // last_updated_unix
        64; // reserved

    /// Initialize a new participant
    pub fn initialize(
        &mut self,
        giveaway: Pubkey,
        wallet: Pubkey,
        commitment_hash: [u8; 32],
        proof_text: String,
        current_time: i64,
        participant_index: u64,
    ) {
        self.giveaway = giveaway;
        self.wallet = wallet;
        self.commitment_hash = commitment_hash;
        self.proof_text = proof_text;
        self.attested_uploaded = false;
        self.attested_at_unix = 0;
        self.reveal_included = false;
        self.disqualified = false;
        self.disqualification_reason = 0;
        self.disqualified_at_unix = 0;
        self.created_at_unix = current_time;
        self.last_updated_unix = current_time;
        self.reserved = [0; 64];
        self.reserved[Self::PARTICIPANT_INDEX_OFFSET
            ..Self::PARTICIPANT_INDEX_OFFSET + Self::PARTICIPANT_INDEX_LEN]
            .copy_from_slice(&participant_index.to_le_bytes());
    }

    /// Update participant entry
    pub fn update(&mut self, commitment_hash: [u8; 32], proof_text: String, current_time: i64) {
        self.commitment_hash = commitment_hash;
        self.proof_text = proof_text;
        self.last_updated_unix = current_time;
    }

    /// Mark as attested
    pub fn mark_attested(&mut self, current_time: i64) {
        self.attested_uploaded = true;
        self.attested_at_unix = current_time;
        self.last_updated_unix = current_time;
    }

    /// Mark reveal as included
    pub fn mark_reveal_included(&mut self, current_time: i64) {
        self.reveal_included = true;
        self.last_updated_unix = current_time;
    }

    pub fn include_verified_reveal(&mut self, digest: [u8; 32], current_time: i64) {
        self.reserved
            [Self::REVEAL_DIGEST_OFFSET..Self::REVEAL_DIGEST_OFFSET + Self::REVEAL_DIGEST_LEN]
            .copy_from_slice(&digest);
        self.mark_reveal_included(current_time);
    }

    pub fn reveal_digest(&self) -> [u8; 32] {
        let mut digest = [0u8; 32];
        digest.copy_from_slice(
            &self.reserved
                [Self::REVEAL_DIGEST_OFFSET..Self::REVEAL_DIGEST_OFFSET + Self::REVEAL_DIGEST_LEN],
        );
        digest
    }

    pub fn participant_index(&self) -> u64 {
        let mut bytes = [0u8; Self::PARTICIPANT_INDEX_LEN];
        bytes.copy_from_slice(
            &self.reserved[Self::PARTICIPANT_INDEX_OFFSET
                ..Self::PARTICIPANT_INDEX_OFFSET + Self::PARTICIPANT_INDEX_LEN],
        );
        u64::from_le_bytes(bytes)
    }

    /// Disqualify participant
    pub fn disqualify(&mut self, reason_code: u8, current_time: i64) {
        self.disqualified = true;
        self.disqualification_reason = reason_code;
        self.disqualified_at_unix = current_time;
        self.last_updated_unix = current_time;
    }

    /// Check if participant is eligible for winner selection
    pub fn is_eligible(&self) -> bool {
        !self.disqualified && self.reveal_included
    }

    pub fn finalization_version(&self) -> u32 {
        let mut bytes = [0u8; Self::FINALIZATION_VERSION_LEN];
        bytes.copy_from_slice(
            &self.reserved[Self::FINALIZATION_VERSION_OFFSET
                ..Self::FINALIZATION_VERSION_OFFSET + Self::FINALIZATION_VERSION_LEN],
        );
        u32::from_le_bytes(bytes)
    }

    pub fn mark_finalized_for_version(&mut self, recompute_version: u32, current_time: i64) {
        self.reserved[Self::FINALIZATION_VERSION_OFFSET
            ..Self::FINALIZATION_VERSION_OFFSET + Self::FINALIZATION_VERSION_LEN]
            .copy_from_slice(&recompute_version.to_le_bytes());
        self.last_updated_unix = current_time;
    }

    pub fn processed_in_selection_pass(&self, version: u32, pass: u16) -> bool {
        let mut version_bytes = [0u8; 4];
        version_bytes.copy_from_slice(
            &self.reserved[Self::SELECTION_VERSION_OFFSET..Self::SELECTION_VERSION_OFFSET + 4],
        );
        let mut pass_bytes = [0u8; 2];
        pass_bytes.copy_from_slice(
            &self.reserved[Self::SELECTION_PASS_OFFSET..Self::SELECTION_PASS_OFFSET + 2],
        );
        u32::from_le_bytes(version_bytes) == version && u16::from_le_bytes(pass_bytes) == pass
    }

    pub fn mark_selection_pass(&mut self, version: u32, pass: u16, current_time: i64) {
        self.reserved[Self::SELECTION_VERSION_OFFSET..Self::SELECTION_VERSION_OFFSET + 4]
            .copy_from_slice(&version.to_le_bytes());
        self.reserved[Self::SELECTION_PASS_OFFSET..Self::SELECTION_PASS_OFFSET + 2]
            .copy_from_slice(&pass.to_le_bytes());
        self.last_updated_unix = current_time;
    }

    pub fn payout_version(&self) -> u32 {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(
            &self.reserved[Self::PAYOUT_VERSION_OFFSET..Self::PAYOUT_VERSION_OFFSET + 4],
        );
        u32::from_le_bytes(bytes)
    }

    pub fn mark_paid_for_version(&mut self, version: u32, current_time: i64) {
        self.reserved[Self::PAYOUT_VERSION_OFFSET..Self::PAYOUT_VERSION_OFFSET + 4]
            .copy_from_slice(&version.to_le_bytes());
        self.last_updated_unix = current_time;
    }

    /// Validate proof text length
    pub fn validate_proof_text(proof_text: &str) -> bool {
        proof_text.len() <= MAX_PROOF_TEXT_LEN
    }

    /// Validate commitment hash (must be non-zero)
    pub fn validate_commitment_hash(commitment_hash: &[u8; 32]) -> bool {
        commitment_hash != &[0u8; 32]
    }

    /// Validate disqualification reason code
    pub fn validate_reason_code(reason_code: u8) -> bool {
        matches!(
            reason_code,
            crate::constants::disqualification_reasons::INCOMPLETE_TASKS
                | crate::constants::disqualification_reasons::INVALID_PROOF
                | crate::constants::disqualification_reasons::TERMS_VIOLATION
                | crate::constants::disqualification_reasons::DUPLICATE_ENTRY
                | crate::constants::disqualification_reasons::OTHER
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_canonical_index_and_verified_reveal_digest_in_reserved_bytes() {
        let mut participant = Participant {
            giveaway: Pubkey::default(),
            wallet: Pubkey::default(),
            commitment_hash: [0; 32],
            proof_text: String::new(),
            attested_uploaded: false,
            attested_at_unix: 0,
            reveal_included: false,
            disqualified: false,
            disqualification_reason: 0,
            disqualified_at_unix: 0,
            created_at_unix: 0,
            last_updated_unix: 0,
            reserved: [0; 64],
        };
        participant.initialize(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            [7; 32],
            "proof".to_string(),
            10,
            42,
        );
        participant.include_verified_reveal([9; 32], 11);

        assert_eq!(participant.participant_index(), 42);
        assert_eq!(participant.reveal_digest(), [9; 32]);
        assert!(participant.reveal_included);
    }
}
