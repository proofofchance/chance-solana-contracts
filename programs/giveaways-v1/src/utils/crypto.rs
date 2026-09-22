//! # Cryptographic Utilities
//!
//! Proof-of-chance reveal verification, deterministic winner selection, and
//! indexed merkle tree helpers for giveaway v1.

use crate::{constants::*, error::GiveawayError};
use anchor_lang::prelude::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinnerEntry {
    pub index: u32,
    pub recipient: Pubkey,
    pub amount: u64,
}

/// Build canonical reveal plaintext: lucky_words || 0x1f || salt.
pub fn build_reveal_plaintext(lucky_words: &str, salt: &[u8]) -> Result<Vec<u8>> {
    require!(
        lucky_words.len() <= MAX_LUCKY_WORDS_LEN && salt.len() <= MAX_SALT_LEN,
        GiveawayError::TextTooLong
    );

    let mut plaintext = Vec::with_capacity(lucky_words.len() + 1 + salt.len());
    plaintext.extend_from_slice(lucky_words.as_bytes());
    plaintext.push(REVEAL_SEPARATOR);
    plaintext.extend_from_slice(salt);
    Ok(plaintext)
}

/// Verify proof-of-chance reveal against commitment.
pub fn verify_reveal(commitment_hash: &[u8; 32], lucky_words: &str, salt: &[u8]) -> Result<bool> {
    let plaintext = build_reveal_plaintext(lucky_words, salt)?;
    let computed_hash: [u8; 32] = Sha256::digest(&plaintext).into();
    Ok(computed_hash == *commitment_hash)
}

/// Domain-separated digest for one reveal. This is the entropy input, not the
/// bare commitment hash.
pub fn compute_reveal_digest(wallet: &Pubkey, plaintext: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_REVEAL_DOMAIN_V1);
    hasher.update(wallet.to_bytes());
    hasher.update((plaintext.len() as u32).to_le_bytes());
    hasher.update(plaintext);
    hasher.finalize().into()
}

/// XOR reveal digests into a batch-order-independent aggregate.
pub fn xor_reveal_digests(initial: [u8; 32], digests: &[[u8; 32]]) -> [u8; 32] {
    let mut aggregate_hash = initial;
    for digest in digests {
        for (idx, byte) in digest.iter().enumerate() {
            aggregate_hash[idx] ^= byte;
        }
    }
    aggregate_hash
}

/// Compute the deterministic seed used for the winner draw.
pub fn compute_winner_seed(
    giveaway_id: u64,
    aggregate_hash: [u8; 32],
    eligible_participants_sorted: &[Pubkey],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_SEED_DOMAIN_V1);
    hasher.update(giveaway_id.to_le_bytes());
    hasher.update((eligible_participants_sorted.len() as u64).to_le_bytes());
    hasher.update(aggregate_hash);

    let mut seed: [u8; 32] = hasher.finalize().into();
    for participant in eligible_participants_sorted {
        let mut round_hasher = Sha256::new();
        round_hasher.update(seed);
        round_hasher.update(participant.to_bytes());
        seed = round_hasher.finalize().into();
    }

    seed
}

/// Compute the base seed used by chunked top-K finalization.
pub fn compute_finalization_seed(giveaway_id: u64, aggregate_hash: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_FINALIZATION_SEED_DOMAIN_V2);
    hasher.update(giveaway_id.to_le_bytes());
    hasher.update(aggregate_hash);
    hasher.finalize().into()
}

/// Bind the final seed to the immutable eligible participant commitment.
pub fn compute_finalization_seed_v2(
    giveaway_id: u64,
    aggregate_hash: [u8; 32],
    eligible_count: u64,
    participants_commitment: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_FINALIZATION_SEED_DOMAIN_V2);
    hasher.update(giveaway_id.to_le_bytes());
    hasher.update(eligible_count.to_le_bytes());
    hasher.update(aggregate_hash);
    hasher.update(participants_commitment);
    hasher.finalize().into()
}

/// Protocol-v3 seed bound to the canonical participant-index reveal commitment.
pub fn compute_finalization_seed_v3(
    giveaway_id: u64,
    aggregate_hash: [u8; 32],
    eligible_count: u64,
    participants_commitment: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_FINALIZATION_SEED_DOMAIN_V3);
    hasher.update(giveaway_id.to_le_bytes());
    hasher.update(eligible_count.to_le_bytes());
    hasher.update(aggregate_hash);
    hasher.update(participants_commitment);
    hasher.finalize().into()
}

/// Compute one participant's deterministic ranking key.
pub fn compute_candidate_rank(seed: &[u8; 32], participant: &Pubkey) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(GIVEAWAY_RANK_DOMAIN_V2);
    hasher.update(seed);
    hasher.update(participant.to_bytes());
    hasher.finalize().into()
}

/// Canonical radix key: domain-separated rank followed by wallet bytes.
pub fn compute_candidate_key(seed: &[u8; 32], participant: &Pubkey) -> [u8; 64] {
    let rank = compute_candidate_rank(seed, participant);
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&rank);
    key[32..].copy_from_slice(participant.as_ref());
    key
}

pub fn compute_participant_commitment_leaf(
    participant_index: u64,
    participant: &Pubkey,
    reveal_digest: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"GIVEAWAY_PARTICIPANT_V3");
    hasher.update(participant_index.to_le_bytes());
    hasher.update(participant.as_ref());
    hasher.update(reveal_digest);
    hasher.finalize().into()
}

pub fn compute_threshold_commitment(threshold_key: &[u8; 64], winners_count: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"GIVEAWAY_THRESHOLD_V2");
    hasher.update(threshold_key);
    hasher.update(winners_count.to_le_bytes());
    hasher.finalize().into()
}

/// Select winners without replacement from a wallet-sorted eligible pool.
pub fn select_winners(
    seed: &[u8; 32],
    eligible_participants_sorted: &[Pubkey],
    number_of_winners: u32,
) -> Vec<Pubkey> {
    if eligible_participants_sorted.is_empty() || number_of_winners == 0 {
        return Vec::new();
    }

    let mut pool = eligible_participants_sorted.to_vec();
    let mut winners = Vec::new();
    let target = usize::min(number_of_winners as usize, pool.len());

    for round in 0..target {
        let mut hasher = Sha256::new();
        hasher.update(GIVEAWAY_DRAW_DOMAIN_V1);
        hasher.update(seed);
        hasher.update((round as u64).to_le_bytes());
        let digest = hasher.finalize();

        let mut first_16 = [0u8; 16];
        first_16.copy_from_slice(&digest[..16]);
        let selected_idx = (u128::from_le_bytes(first_16) % pool.len() as u128) as usize;
        winners.push(pool.remove(selected_idx));
    }

    winners
}

/// Create indexed merkle leaf for a winner.
pub fn create_winner_leaf(index: u32, winner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut leaf = Vec::with_capacity(44);
    leaf.extend_from_slice(&index.to_le_bytes());
    leaf.extend_from_slice(winner.as_ref());
    leaf.extend_from_slice(&amount.to_le_bytes());
    leaf
}

/// Build indexed merkle tree and return root.
pub fn build_merkle_tree(winners: &[WinnerEntry]) -> Result<[u8; 32]> {
    if winners.is_empty() {
        return Err(GiveawayError::NoEligibleParticipants.into());
    }

    let mut leaves: Vec<[u8; 32]> = winners
        .iter()
        .map(|winner| {
            Sha256::digest(create_winner_leaf(
                winner.index,
                &winner.recipient,
                winner.amount,
            ))
            .into()
        })
        .collect();

    while leaves.len() > 1 {
        let mut next_level = Vec::new();
        for chunk in leaves.chunks(2) {
            let mut hasher = Sha256::new();
            hasher.update(chunk[0]);
            if chunk.len() == 2 {
                hasher.update(chunk[1]);
            } else {
                hasher.update(chunk[0]);
            }
            next_level.push(hasher.finalize().into());
        }
        leaves = next_level;
    }

    Ok(leaves[0])
}

/// Verify an indexed merkle proof.
pub fn verify_merkle_proof(
    index: u32,
    winner: &Pubkey,
    amount: u64,
    proof: &[Vec<u8>],
    root: &[u8; 32],
) -> bool {
    let mut current_hash: [u8; 32] =
        Sha256::digest(create_winner_leaf(index, winner, amount)).into();
    let mut path_index = index;

    for sibling in proof {
        if sibling.len() != 32 {
            return false;
        }
        let mut sibling_hash = [0u8; 32];
        sibling_hash.copy_from_slice(sibling);

        let mut hasher = Sha256::new();
        if path_index % 2 == 0 {
            hasher.update(current_hash);
            hasher.update(sibling_hash);
        } else {
            hasher.update(sibling_hash);
            hasher.update(current_hash);
        }
        current_hash = hasher.finalize().into();
        path_index /= 2;
    }

    &current_hash == root
}

/// Parse reveal data from plaintext using the canonical separator.
pub fn parse_reveal_data(plaintext: &[u8]) -> Result<(String, Vec<u8>)> {
    let separator_idx = plaintext
        .iter()
        .position(|byte| *byte == REVEAL_SEPARATOR)
        .ok_or(GiveawayError::InvalidReveal)?;

    let lucky_words = core::str::from_utf8(&plaintext[..separator_idx])
        .map_err(|_| GiveawayError::InvalidReveal)?
        .to_string();
    let salt = plaintext[separator_idx + 1..].to_vec();

    build_reveal_plaintext(&lucky_words, &salt)?;
    Ok((lucky_words, salt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_commitment_uses_unit_separator() {
        let lucky_words = "fortune";
        let salt = b"abc123";
        let plaintext = build_reveal_plaintext(lucky_words, salt).unwrap();
        assert_eq!(plaintext, b"fortune\x1fabc123");

        let commitment: [u8; 32] = Sha256::digest(&plaintext).into();
        assert!(verify_reveal(&commitment, lucky_words, salt).unwrap());
    }

    #[test]
    fn reveal_digest_aggregate_is_order_independent() {
        let wallet_a = Pubkey::new_unique();
        let wallet_b = Pubkey::new_unique();
        let digest_a = compute_reveal_digest(&wallet_a, b"one\x1fa");
        let digest_b = compute_reveal_digest(&wallet_b, b"two\x1fb");

        assert_eq!(
            xor_reveal_digests([0u8; 32], &[digest_a, digest_b]),
            xor_reveal_digests([0u8; 32], &[digest_b, digest_a])
        );
    }

    #[test]
    fn winner_selection_is_deterministic_without_replacement() {
        let mut participants = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        participants.sort_by_key(|pubkey| pubkey.to_bytes());
        let seed = compute_winner_seed(7, [9u8; 32], &participants);

        let winners_a = select_winners(&seed, &participants, 3);
        let winners_b = select_winners(&seed, &participants, 3);
        assert_eq!(winners_a, winners_b);

        let unique = winners_a
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), winners_a.len());
    }

    #[test]
    fn candidate_rank_is_deterministic_and_wallet_specific() {
        let wallet_a = Pubkey::new_unique();
        let wallet_b = Pubkey::new_unique();
        let seed = compute_finalization_seed(1, [3u8; 32]);

        let rank_a = compute_candidate_rank(&seed, &wallet_a);
        assert_eq!(rank_a, compute_candidate_rank(&seed, &wallet_a));
        assert_ne!(rank_a, compute_candidate_rank(&seed, &wallet_b));
    }

    #[test]
    fn single_winner_merkle_root_matches_verifier() {
        let winner = Pubkey::new_unique();
        let entry = WinnerEntry {
            index: 0,
            recipient: winner,
            amount: 42,
        };
        let root = build_merkle_tree(&[entry]).expect("root");

        assert!(verify_merkle_proof(0, &winner, 42, &[], &root));
        assert!(!verify_merkle_proof(1, &winner, 42, &[], &root));
    }
}
