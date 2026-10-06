//! Participant-set validation for winner computation.

use std::collections::HashSet;

use crate::{constants::PARTICIPANT_SEED, error::GiveawayError, state::Participant};
use anchor_lang::prelude::*;

/// Load the canonical reveal-included participant set for winner computation.
///
/// The caller supplies participant accounts through `remaining_accounts`; this
/// function makes that dynamic list safe by verifying ownership, PDA seeds,
/// giveaway linkage, and uniqueness before any wallet enters the draw pool.
pub fn collect_canonical_eligible_wallets<'info>(
    program_id: &Pubkey,
    giveaway: Pubkey,
    expected_included_reveals: u64,
    remaining_accounts: &[AccountInfo<'info>],
) -> Result<Vec<Pubkey>> {
    let mut seen_accounts = HashSet::new();
    let mut seen_wallets = HashSet::new();
    let mut included_reveals = 0u64;
    let mut eligible_wallets = Vec::new();

    for account_info in remaining_accounts.iter() {
        require_eq!(
            account_info.owner,
            program_id,
            GiveawayError::InvalidProgram
        );

        let account_key = account_info.key();
        require!(
            seen_accounts.insert(account_key),
            GiveawayError::InvalidAccount
        );

        let mut data = &account_info.data.borrow()[..];
        let participant = Participant::try_deserialize(&mut data)
            .map_err(|_| GiveawayError::AccountNotInitialized)?;

        require_keys_eq!(
            participant.giveaway,
            giveaway,
            GiveawayError::InvalidAccount
        );

        let (expected_participant_pda, _) = Pubkey::find_program_address(
            &[
                PARTICIPANT_SEED,
                giveaway.as_ref(),
                participant.wallet.as_ref(),
            ],
            program_id,
        );
        require_keys_eq!(
            account_key,
            expected_participant_pda,
            GiveawayError::InvalidAccount
        );

        require!(
            seen_wallets.insert(participant.wallet),
            GiveawayError::InvalidAccount
        );

        if participant.reveal_included {
            included_reveals = included_reveals
                .checked_add(1)
                .ok_or(GiveawayError::MathOverflow)?;
        }
        if participant.is_eligible() {
            eligible_wallets.push(participant.wallet);
        }
    }

    require_eq!(
        included_reveals,
        expected_included_reveals,
        GiveawayError::MissingAttestedParticipants
    );

    eligible_wallets.sort_by_key(|pubkey| pubkey.to_bytes());
    Ok(eligible_wallets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Participant;
    use std::io::Cursor;

    fn participant_for(giveaway: Pubkey, wallet: Pubkey) -> Participant {
        Participant {
            giveaway,
            wallet,
            commitment_hash: [7u8; 32],
            proof_text: "proof".to_string(),
            attested_uploaded: true,
            attested_at_unix: 10,
            reveal_included: true,
            disqualified: false,
            disqualification_reason: 0,
            disqualified_at_unix: 0,
            created_at_unix: 1,
            last_updated_unix: 10,
            reserved: [0; 64],
        }
    }

    fn write_participant(data: &mut [u8], participant: &Participant) {
        let mut writer = Cursor::new(data);
        participant.try_serialize(&mut writer).unwrap();
    }

    #[test]
    fn accepts_distinct_owned_participant_pdas() {
        let program_id = crate::ID;
        let giveaway = Pubkey::new_unique();
        let wallet_a = Pubkey::new_unique();
        let wallet_b = Pubkey::new_unique();
        let (participant_a_key, _) = Pubkey::find_program_address(
            &[PARTICIPANT_SEED, giveaway.as_ref(), wallet_a.as_ref()],
            &program_id,
        );
        let (participant_b_key, _) = Pubkey::find_program_address(
            &[PARTICIPANT_SEED, giveaway.as_ref(), wallet_b.as_ref()],
            &program_id,
        );
        let participant_a = participant_for(giveaway, wallet_a);
        let participant_b = participant_for(giveaway, wallet_b);
        let mut lamports_a = 0;
        let mut lamports_b = 0;
        let mut data_a = vec![0u8; Participant::MAX_SIZE];
        let mut data_b = vec![0u8; Participant::MAX_SIZE];
        write_participant(&mut data_a, &participant_a);
        write_participant(&mut data_b, &participant_b);
        let account_a = AccountInfo::new(
            &participant_a_key,
            false,
            true,
            &mut lamports_a,
            &mut data_a,
            &program_id,
            false,
            0,
        );
        let account_b = AccountInfo::new(
            &participant_b_key,
            false,
            true,
            &mut lamports_b,
            &mut data_b,
            &program_id,
            false,
            0,
        );

        let wallets =
            collect_canonical_eligible_wallets(&program_id, giveaway, 2, &[account_a, account_b])
                .unwrap();

        assert_eq!(wallets.len(), 2);
        assert!(wallets.contains(&wallet_a));
        assert!(wallets.contains(&wallet_b));
    }

    #[test]
    fn rejects_duplicate_participant_accounts() {
        let program_id = crate::ID;
        let giveaway = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let (participant_key, _) = Pubkey::find_program_address(
            &[PARTICIPANT_SEED, giveaway.as_ref(), wallet.as_ref()],
            &program_id,
        );
        let participant = participant_for(giveaway, wallet);
        let mut lamports = 0;
        let mut data = vec![0u8; Participant::MAX_SIZE];
        write_participant(&mut data, &participant);
        let account = AccountInfo::new(
            &participant_key,
            false,
            true,
            &mut lamports,
            &mut data,
            &program_id,
            false,
            0,
        );

        assert!(collect_canonical_eligible_wallets(
            &program_id,
            giveaway,
            2,
            &[account.clone(), account]
        )
        .is_err());
    }

    #[test]
    fn rejects_wrong_owner_even_if_data_deserializes() {
        let program_id = crate::ID;
        let wrong_owner = Pubkey::new_unique();
        let giveaway = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let (participant_key, _) = Pubkey::find_program_address(
            &[PARTICIPANT_SEED, giveaway.as_ref(), wallet.as_ref()],
            &program_id,
        );
        let participant = participant_for(giveaway, wallet);
        let mut lamports = 0;
        let mut data = vec![0u8; Participant::MAX_SIZE];
        write_participant(&mut data, &participant);
        let account = AccountInfo::new(
            &participant_key,
            false,
            true,
            &mut lamports,
            &mut data,
            &wrong_owner,
            false,
            0,
        );

        assert!(collect_canonical_eligible_wallets(&program_id, giveaway, 1, &[account]).is_err());
    }

    #[test]
    fn rejects_valid_data_at_wrong_pda() {
        let program_id = crate::ID;
        let giveaway = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let wrong_key = Pubkey::new_unique();
        let participant = participant_for(giveaway, wallet);
        let mut lamports = 0;
        let mut data = vec![0u8; Participant::MAX_SIZE];
        write_participant(&mut data, &participant);
        let account = AccountInfo::new(
            &wrong_key,
            false,
            true,
            &mut lamports,
            &mut data,
            &program_id,
            false,
            0,
        );

        assert!(collect_canonical_eligible_wallets(&program_id, giveaway, 1, &[account]).is_err());
    }
}
