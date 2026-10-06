//! Fixed-size protocol-v2 giveaway settlement root.

use crate::error::GiveawayError;
use anchor_lang::prelude::*;

#[account]
pub struct WinnersLedger {
    pub giveaway: Pubkey,
    pub protocol_version: u16,
    pub threshold_key: [u8; 64],
    pub winners_commitment: [u8; 32],
    pub winners_count: u32,
    pub total_payout_lamports: u64,
    pub per_winner_lamports: u64,
    pub paid_count: u32,
    pub batches_processed: u32,
    pub recompute_version: u32,
    pub locked: bool,
    pub computed_at_unix: i64,
    pub locked_at_unix: i64,
    pub settlement_started_at_unix: i64,
    pub settlement_completed_at_unix: i64,
    pub reserved: [u8; 128],
}

impl WinnersLedger {
    pub const SIZE: usize = 8 + 32 + 2 + 64 + 32 + 4 + 8 + 8 + 4 + 4 + 4 + 1 + 8 + 8 + 8 + 8 + 128;

    pub fn calculate_size(_winners_count: u32) -> usize {
        Self::SIZE
    }

    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        &mut self,
        giveaway: Pubkey,
        threshold_key: [u8; 64],
        winners_commitment: [u8; 32],
        winners_count: u32,
        total_payout_lamports: u64,
        per_winner_lamports: u64,
        recompute_version: u32,
        current_time: i64,
    ) {
        self.giveaway = giveaway;
        self.protocol_version = 2;
        self.threshold_key = threshold_key;
        self.winners_commitment = winners_commitment;
        self.winners_count = winners_count;
        self.total_payout_lamports = total_payout_lamports;
        self.per_winner_lamports = per_winner_lamports;
        self.paid_count = 0;
        self.batches_processed = 0;
        self.recompute_version = recompute_version;
        self.locked = false;
        self.computed_at_unix = current_time;
        self.locked_at_unix = 0;
        self.settlement_started_at_unix = 0;
        self.settlement_completed_at_unix = 0;
        self.reserved = [0; 128];
    }

    pub fn lock(&mut self, current_time: i64) {
        self.locked = true;
        self.locked_at_unix = current_time;
    }
    pub fn start_settlement(&mut self, current_time: i64) {
        if self.settlement_started_at_unix == 0 {
            self.settlement_started_at_unix = current_time;
        }
    }
    pub fn mark_winner_paid(&mut self) -> Result<()> {
        require!(
            self.paid_count < self.winners_count,
            GiveawayError::AlreadyPaid
        );
        self.paid_count = self
            .paid_count
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        Ok(())
    }
    pub fn mark_batch_processed(&mut self) -> Result<()> {
        self.batches_processed = self
            .batches_processed
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        Ok(())
    }
    pub fn is_settlement_complete(&self) -> bool {
        self.paid_count == self.winners_count
    }
    pub fn complete_settlement(&mut self, current_time: i64) {
        if self.is_settlement_complete() {
            self.settlement_completed_at_unix = current_time;
        }
    }
    pub fn get_unpaid_count(&self) -> u32 {
        self.winners_count.saturating_sub(self.paid_count)
    }
    pub fn validate_winner_index(&self, winner_index: u32) -> bool {
        winner_index < self.winners_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn below_one_kib(size: usize) -> bool {
        size < 1024
    }
    #[test]
    fn account_size_is_fixed() {
        assert_eq!(
            WinnersLedger::calculate_size(1),
            WinnersLedger::calculate_size(1000)
        );
        assert!(below_one_kib(WinnersLedger::SIZE));
    }
}
