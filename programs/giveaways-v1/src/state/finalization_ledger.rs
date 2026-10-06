//! Fixed-size protocol-v2 radix finalization state.

use crate::error::GiveawayError;
use anchor_lang::prelude::*;
use sha2::{Digest, Sha256};

pub const FINALIZATION_PROTOCOL_VERSION: u16 = 4;
pub const PHASE_AGGREGATING: u8 = 0;
pub const PHASE_RADIX: u8 = 1;
pub const PHASE_RESOLVING: u8 = 2;
pub const PHASE_EMITTING: u8 = 3;
pub const PHASE_COMPLETED: u8 = 4;
pub const CANDIDATE_KEY_LEN: usize = 64;

#[account(zero_copy(unsafe))]
#[repr(C)]
pub struct FinalizationLedger {
    pub giveaway: Pubkey,
    pub protocol_version: u16,
    pub recompute_version: u32,
    pub phase: u8,
    pub required_count: u64,
    pub processed_count: u64,
    pub eligible_count: u64,
    pub target_winners: u32,
    pub seed: [u8; 32],
    pub participants_commitment: [u8; 32],
    pub prefix: [u8; CANDIDATE_KEY_LEN],
    pub prefix_len: u8,
    pub remaining_rank: u64,
    pub histogram: [u64; 256],
    pub threshold_key: [u8; CANDIDATE_KEY_LEN],
    pub threshold_found: u8,
    pub completed: u8,
    pub started_at_unix: i64,
    pub completed_at_unix: i64,
    pub reserved: [u8; 64],
}

impl FinalizationLedger {
    const EMITTED_COUNT_OFFSET: usize = 0;
    const EMITTED_COUNT_LEN: usize = 4;
    pub const SIZE: usize = 8 + core::mem::size_of::<Self>();

    pub fn calculate_size(_target_winners: u32) -> usize {
        Self::SIZE
    }

    pub fn initialize(
        &mut self,
        giveaway: Pubkey,
        recompute_version: u32,
        required_count: u64,
        current_time: i64,
    ) {
        self.giveaway = giveaway;
        self.protocol_version = FINALIZATION_PROTOCOL_VERSION;
        self.recompute_version = recompute_version;
        self.phase = PHASE_AGGREGATING;
        self.required_count = required_count;
        self.processed_count = 0;
        self.eligible_count = 0;
        self.target_winners = 0;
        self.seed = [0; 32];
        self.participants_commitment = [0; 32];
        self.prefix = [0; CANDIDATE_KEY_LEN];
        self.prefix_len = 0;
        self.remaining_rank = 0;
        self.histogram = [0; 256];
        self.threshold_key = [0; CANDIDATE_KEY_LEN];
        self.threshold_found = 0;
        self.completed = 0;
        self.started_at_unix = current_time;
        self.completed_at_unix = 0;
        self.reserved = [0; 64];
    }

    pub fn record_processed(&mut self) -> Result<()> {
        self.processed_count = self
            .processed_count
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        Ok(())
    }

    pub fn record_eligible(&mut self, commitment_leaf: [u8; 32]) -> Result<()> {
        self.eligible_count = self
            .eligible_count
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        let mut hasher = Sha256::new();
        hasher.update(b"GIVEAWAY_POOL_V3");
        hasher.update(self.participants_commitment);
        hasher.update(commitment_leaf);
        self.participants_commitment = hasher.finalize().into();
        Ok(())
    }

    pub fn begin_radix(&mut self, seed: [u8; 32], target_winners: u32) {
        self.phase = PHASE_RADIX;
        self.seed = seed;
        self.target_winners = target_winners;
        self.remaining_rank = target_winners as u64;
        self.reset_pass();
    }

    pub fn record_radix_key(&mut self, key: &[u8; CANDIDATE_KEY_LEN]) -> Result<()> {
        if self.prefix_len as usize >= CANDIDATE_KEY_LEN
            || key[..self.prefix_len as usize] != self.prefix[..self.prefix_len as usize]
        {
            return Ok(());
        }
        let bucket = key[self.prefix_len as usize] as usize;
        self.histogram[bucket] = self.histogram[bucket]
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        Ok(())
    }

    pub fn finish_radix_pass(&mut self) -> Result<()> {
        require!(self.remaining_rank > 0, GiveawayError::InvalidInstruction);
        let mut before = 0u64;
        for (bucket, count) in self.histogram.iter().copied().enumerate() {
            let through = before
                .checked_add(count)
                .ok_or(GiveawayError::MathOverflow)?;
            if self.remaining_rank <= through {
                self.prefix[self.prefix_len as usize] = bucket as u8;
                self.prefix_len = self
                    .prefix_len
                    .checked_add(1)
                    .ok_or(GiveawayError::MathOverflow)?;
                self.remaining_rank = self.remaining_rank.saturating_sub(before);
                if count == 1 || self.prefix_len as usize == CANDIDATE_KEY_LEN {
                    self.phase = PHASE_RESOLVING;
                }
                self.reset_pass();
                return Ok(());
            }
            before = through;
        }
        err!(GiveawayError::InvalidInstruction)
    }

    pub fn matches_prefix(&self, key: &[u8; CANDIDATE_KEY_LEN]) -> bool {
        key[..self.prefix_len as usize] == self.prefix[..self.prefix_len as usize]
    }

    pub fn resolve_threshold(&mut self, key: [u8; CANDIDATE_KEY_LEN]) -> Result<()> {
        require!(self.threshold_found == 0, GiveawayError::InvalidAccount);
        self.threshold_key = key;
        self.threshold_found = 1;
        Ok(())
    }

    pub fn begin_emitting(&mut self) {
        self.phase = PHASE_EMITTING;
        self.reserved
            [Self::EMITTED_COUNT_OFFSET..Self::EMITTED_COUNT_OFFSET + Self::EMITTED_COUNT_LEN]
            .copy_from_slice(&0u32.to_le_bytes());
        self.reset_pass();
    }

    pub fn emitted_count(&self) -> u32 {
        let mut bytes = [0u8; Self::EMITTED_COUNT_LEN];
        bytes.copy_from_slice(
            &self.reserved
                [Self::EMITTED_COUNT_OFFSET..Self::EMITTED_COUNT_OFFSET + Self::EMITTED_COUNT_LEN],
        );
        u32::from_le_bytes(bytes)
    }

    pub fn record_emitted(&mut self) -> Result<u32> {
        let winner_index = self.emitted_count();
        let next = winner_index
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        self.reserved
            [Self::EMITTED_COUNT_OFFSET..Self::EMITTED_COUNT_OFFSET + Self::EMITTED_COUNT_LEN]
            .copy_from_slice(&next.to_le_bytes());
        Ok(winner_index)
    }

    pub fn complete(&mut self, current_time: i64) {
        self.phase = PHASE_COMPLETED;
        self.completed = 1;
        self.completed_at_unix = current_time;
    }

    pub fn reset_pass(&mut self) {
        self.processed_count = 0;
        self.histogram = [0; 256];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::compute_candidate_key;

    fn below_eight_kib(size: usize) -> bool {
        size < 8 * 1024
    }

    #[test]
    fn account_is_fixed_and_below_eight_kib() {
        assert_eq!(
            FinalizationLedger::calculate_size(1),
            FinalizationLedger::calculate_size(1000)
        );
        assert!(below_eight_kib(FinalizationLedger::SIZE));
    }

    #[test]
    fn radix_selects_bucket_containing_kth_key() {
        let mut root = FinalizationLedger {
            giveaway: Pubkey::default(),
            protocol_version: 0,
            recompute_version: 0,
            phase: 0,
            required_count: 0,
            processed_count: 0,
            eligible_count: 0,
            target_winners: 0,
            seed: [0; 32],
            participants_commitment: [0; 32],
            prefix: [0; 64],
            prefix_len: 0,
            remaining_rank: 0,
            histogram: [0; 256],
            threshold_key: [0; 64],
            threshold_found: 0,
            completed: 0,
            started_at_unix: 0,
            completed_at_unix: 0,
            reserved: [0; 64],
        };
        root.initialize(Pubkey::new_unique(), 1, 3, 1);
        root.begin_radix([1; 32], 2);
        for first in [1u8, 7, 9] {
            let mut key = [0u8; 64];
            key[0] = first;
            root.record_radix_key(&key).unwrap();
        }
        root.finish_radix_pass().unwrap();
        assert_eq!(root.prefix[0], 7);
        assert_eq!(root.phase, PHASE_RESOLVING);
    }

    #[test]
    fn archival_emission_count_is_bounded_and_persisted() {
        let mut root = FinalizationLedger {
            giveaway: Pubkey::default(),
            protocol_version: 0,
            recompute_version: 0,
            phase: 0,
            required_count: 0,
            processed_count: 0,
            eligible_count: 0,
            target_winners: 0,
            seed: [0; 32],
            participants_commitment: [0; 32],
            prefix: [0; 64],
            prefix_len: 0,
            remaining_rank: 0,
            histogram: [0; 256],
            threshold_key: [0; 64],
            threshold_found: 0,
            completed: 0,
            started_at_unix: 0,
            completed_at_unix: 0,
            reserved: [0; 64],
        };
        root.initialize(Pubkey::new_unique(), 7, 12, 1);
        root.begin_emitting();

        assert_eq!(root.phase, PHASE_EMITTING);
        assert_eq!(root.record_emitted().unwrap(), 0);
        assert_eq!(root.record_emitted().unwrap(), 1);
        assert_eq!(root.emitted_count(), 2);
    }

    #[test]
    fn radix_threshold_selects_exact_top_k_from_fifteen_hundred_candidates() {
        let seed = [7u8; 32];
        let target = 317u32;
        let keys = (0..1_500u32)
            .map(|value| {
                let mut bytes = [0u8; 32];
                bytes[..4].copy_from_slice(&value.to_le_bytes());
                compute_candidate_key(&seed, &Pubkey::new_from_array(bytes))
            })
            .collect::<Vec<_>>();
        let mut root = FinalizationLedger {
            giveaway: Pubkey::default(),
            protocol_version: 0,
            recompute_version: 0,
            phase: 0,
            required_count: 0,
            processed_count: 0,
            eligible_count: 0,
            target_winners: 0,
            seed: [0; 32],
            participants_commitment: [0; 32],
            prefix: [0; 64],
            prefix_len: 0,
            remaining_rank: 0,
            histogram: [0; 256],
            threshold_key: [0; 64],
            threshold_found: 0,
            completed: 0,
            started_at_unix: 0,
            completed_at_unix: 0,
            reserved: [0; 64],
        };
        root.initialize(Pubkey::new_unique(), 1, keys.len() as u64, 1);
        root.eligible_count = keys.len() as u64;
        root.begin_radix(seed, target);
        while root.phase == PHASE_RADIX {
            for key in &keys {
                root.record_radix_key(key).unwrap();
            }
            root.finish_radix_pass().unwrap();
        }
        for key in &keys {
            if root.matches_prefix(key) {
                root.resolve_threshold(*key).unwrap();
            }
        }

        assert_eq!(
            keys.iter()
                .filter(|key| **key <= root.threshold_key)
                .count(),
            target as usize
        );
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(root.threshold_key, sorted[target as usize - 1]);
    }
}
