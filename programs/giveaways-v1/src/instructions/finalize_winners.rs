//! Permissionless fixed-account radix-threshold winner finalization.

use crate::{
    constants::*,
    error::GiveawayError,
    events::GiveawayEvent,
    state::{
        Config, FinalizationLedger, Giveaway, Participant, WinnersLedger,
        FINALIZATION_PROTOCOL_VERSION, PHASE_AGGREGATING, PHASE_COMPLETED, PHASE_EMITTING,
        PHASE_RADIX, PHASE_RESOLVING,
    },
    utils::{
        crypto::{
            compute_candidate_key, compute_finalization_seed_v3,
            compute_participant_commitment_leaf, compute_threshold_commitment,
        },
        pda::assert_pda_owned,
    },
};
use anchor_lang::prelude::*;
use std::io::Cursor;

#[derive(Accounts)]
pub struct FinalizeWinners<'info> {
    #[account()]
    pub config: Account<'info, Config>,
    #[account(mut, constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = !giveaway.winners_locked @ GiveawayError::WinnersLocked,
        constraint = giveaway.config == config.key() @ GiveawayError::InvalidAccount)]
    pub giveaway: Account<'info, Giveaway>,
    /// CHECK: Validated against the giveaway vault.
    #[account(constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount)]
    pub vault: AccountInfo<'info>,
    #[account(init_if_needed, payer = authority, space = WinnersLedger::SIZE,
        seeds = [WINNERS_LEDGER_SEED, giveaway.key().as_ref()], bump)]
    pub winners_ledger: Account<'info, WinnersLedger>,
    #[account(init_if_needed, payer = authority, space = FinalizationLedger::SIZE,
        seeds = [FINALIZATION_LEDGER_SEED, giveaway.key().as_ref()], bump)]
    pub finalization_ledger: AccountLoader<'info, FinalizationLedger>,
    /// Service-provider authority funds and advances every finalization chunk.
    #[account(mut, constraint = authority.key() == giveaway.provider_authority @ GiveawayError::Unauthorized)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn process(ctx: Context<FinalizeWinners>) -> Result<()> {
    let accounts = ctx.accounts;
    let giveaway = &mut accounts.giveaway;
    let root_key = accounts.finalization_ledger.key();
    let root_uninitialized = {
        let root_info = accounts.finalization_ledger.to_account_info();
        let data = root_info.data.borrow();
        data[..8].iter().all(|byte| *byte == 0)
    };
    let mut root_data = if root_uninitialized {
        accounts.finalization_ledger.load_init()?
    } else {
        accounts.finalization_ledger.load_mut()?
    };
    let root = &mut *root_data;
    let clock = Clock::get()?;
    require!(
        clock.unix_timestamp < giveaway.settlement_deadline_unix,
        GiveawayError::DeadlinePassed
    );
    let giveaway_key = giveaway.key();

    require!(
        giveaway.is_ready_for_settlement(clock.unix_timestamp),
        GiveawayError::SettlementNotReady
    );
    require!(
        giveaway.attested_count > 0,
        GiveawayError::NoEligibleParticipants
    );
    if handle_missing_reveals(giveaway, clock.unix_timestamp)? {
        return Ok(());
    }
    if giveaway.attested_reveals_complete() {
        giveaway.uploads_complete = true;
    }

    let generation = giveaway
        .recompute_version
        .checked_add(1)
        .ok_or(GiveawayError::MathOverflow)?;
    if root.giveaway == Pubkey::default() {
        root.initialize(
            giveaway_key,
            generation,
            giveaway.participants_count,
            clock.unix_timestamp,
        );
    } else {
        require_keys_eq!(root.giveaway, giveaway_key, GiveawayError::InvalidAccount);
        require_eq!(
            root.protocol_version,
            FINALIZATION_PROTOCOL_VERSION,
            GiveawayError::InvalidAccount
        );
        require_eq!(
            root.recompute_version,
            generation,
            GiveawayError::InvalidInstruction
        );
        require_eq!(
            root.required_count,
            giveaway.participants_count,
            GiveawayError::InvalidInstruction
        );
        if root.completed != 0 {
            return Ok(());
        }
    }
    require!(
        !ctx.remaining_accounts.is_empty(),
        GiveawayError::InvalidInstruction
    );

    let mut batch_size = 0u32;
    for participant_info in ctx.remaining_accounts.iter() {
        let mut participant = load_participant(ctx.program_id, participant_info, &giveaway_key)?;
        require_eq!(
            participant.participant_index(),
            root.processed_count,
            GiveawayError::InvalidInstruction
        );
        let selected = match root.phase {
            PHASE_AGGREGATING => {
                process_aggregation(root, &mut participant)?;
                None
            }
            PHASE_RADIX => {
                process_radix(root, &mut participant)?;
                None
            }
            PHASE_RESOLVING => {
                process_resolution(root, &mut participant)?;
                None
            }
            PHASE_EMITTING => process_emitting(root, &mut participant)?,
            PHASE_COMPLETED => return err!(GiveawayError::WinnersLocked),
            _ => return err!(GiveawayError::InvalidAccount),
        };
        if let Some((candidate_key, emission_index)) = selected {
            GiveawayEvent::WinnerSelected {
                giveaway_id: giveaway.id,
                giveaway: giveaway_key.to_string(),
                participant: participant.wallet.to_string(),
                participant_account: participant_info.key().to_string(),
                candidate_key: hex::encode(candidate_key),
                recompute_version: root.recompute_version,
                emission_index,
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }
        participant.last_updated_unix = clock.unix_timestamp;
        store_participant(participant_info, &participant)?;
        root.record_processed()?;
        batch_size = batch_size
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
    }
    require!(
        root.processed_count <= root.required_count,
        GiveawayError::InvalidInstruction
    );

    let mut completed = false;
    if root.processed_count == root.required_count {
        match root.phase {
            PHASE_AGGREGATING => {
                let target = u64::from(giveaway.number_of_winners).min(root.eligible_count) as u32;
                if target == 0 {
                    root.target_winners = 0;
                    root.complete(clock.unix_timestamp);
                    completed = true;
                } else {
                    let seed = compute_finalization_seed_v3(
                        giveaway.id,
                        giveaway.poc_aggregate_hash,
                        root.eligible_count,
                        root.participants_commitment,
                    );
                    root.begin_radix(seed, target);
                }
            }
            PHASE_RADIX => root.finish_radix_pass()?,
            PHASE_RESOLVING => {
                require!(
                    root.threshold_found != 0,
                    GiveawayError::NoEligibleParticipants
                );
                root.begin_emitting();
            }
            PHASE_EMITTING => {
                require_eq!(
                    root.emitted_count(),
                    root.target_winners,
                    GiveawayError::InvalidAccount
                );
                root.complete(clock.unix_timestamp);
                completed = true;
            }
            _ => {}
        }
    }

    if completed {
        finalize_settlement_root(
            giveaway,
            &mut accounts.winners_ledger,
            root,
            clock.unix_timestamp,
        )?;
    }

    GiveawayEvent::FinalizationChunkProcessed {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        finalization_ledger: root_key.to_string(),
        recompute_version: root.recompute_version,
        batch_size,
        processed_count: if completed {
            root.required_count
        } else {
            root.processed_count
        },
        required_count: root.required_count,
        eligible_count: root.eligible_count,
        candidate_count: root.target_winners,
        completed,
        timestamp: clock.unix_timestamp,
    }
    .emit();
    Ok(())
}

fn handle_missing_reveals(giveaway: &mut Account<Giveaway>, now: i64) -> Result<bool> {
    if !giveaway.has_missing_attested_reveals() {
        return Ok(false);
    }
    if now >= giveaway.upload_deadline_unix && giveaway.remediation_start_unix == 0 {
        giveaway.begin_remediation(now);
        GiveawayEvent::RevealRemediationBegan {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            included_reveals_count: giveaway.provider_uploaded_count,
            attested_count: giveaway.attested_count,
            remediation_start_unix: giveaway.remediation_start_unix,
            remediation_deadline_unix: giveaway.remediation_deadline_unix,
            timestamp: now,
        }
        .emit();
        return Ok(true);
    }
    err!(GiveawayError::MissingAttestedParticipants)
}

fn load_participant(
    program_id: &Pubkey,
    info: &AccountInfo,
    giveaway: &Pubkey,
) -> Result<Participant> {
    require!(info.is_writable, GiveawayError::AccountNotWritable);
    require_eq!(info.owner, program_id, GiveawayError::InvalidProgram);
    let mut data = &info.data.borrow()[..];
    let participant = Participant::try_deserialize(&mut data)
        .map_err(|_| GiveawayError::AccountNotInitialized)?;
    require_keys_eq!(
        participant.giveaway,
        *giveaway,
        GiveawayError::InvalidAccount
    );
    assert_pda_owned(
        program_id,
        info,
        &[
            PARTICIPANT_SEED,
            giveaway.as_ref(),
            participant.wallet.as_ref(),
        ],
    )?;
    Ok(participant)
}

fn store_participant(info: &AccountInfo, participant: &Participant) -> Result<()> {
    let mut data = info.data.borrow_mut();
    data.fill(0);
    participant.try_serialize(&mut Cursor::new(&mut data[..]))
}

fn process_aggregation(root: &mut FinalizationLedger, participant: &mut Participant) -> Result<()> {
    require!(
        participant.finalization_version() != root.recompute_version,
        GiveawayError::InvalidAccount
    );
    participant.mark_finalized_for_version(root.recompute_version, participant.last_updated_unix);
    if participant.is_eligible() {
        root.record_eligible(compute_participant_commitment_leaf(
            participant.participant_index(),
            &participant.wallet,
            participant.reveal_digest(),
        ))?;
    }
    Ok(())
}

fn process_radix(root: &mut FinalizationLedger, participant: &mut Participant) -> Result<()> {
    let pass = root.prefix_len as u16 + 1;
    require!(
        !participant.processed_in_selection_pass(root.recompute_version, pass),
        GiveawayError::InvalidAccount
    );
    participant.mark_selection_pass(root.recompute_version, pass, participant.last_updated_unix);
    if participant.is_eligible() {
        root.record_radix_key(&compute_candidate_key(&root.seed, &participant.wallet))?;
    }
    Ok(())
}

fn process_resolution(root: &mut FinalizationLedger, participant: &mut Participant) -> Result<()> {
    let pass = 1000u16
        .checked_add(root.prefix_len as u16)
        .ok_or(GiveawayError::MathOverflow)?;
    require!(
        !participant.processed_in_selection_pass(root.recompute_version, pass),
        GiveawayError::InvalidAccount
    );
    participant.mark_selection_pass(root.recompute_version, pass, participant.last_updated_unix);
    if participant.is_eligible() {
        let key = compute_candidate_key(&root.seed, &participant.wallet);
        if root.matches_prefix(&key) {
            root.resolve_threshold(key)?;
        }
    }
    Ok(())
}

fn process_emitting(
    root: &mut FinalizationLedger,
    participant: &mut Participant,
) -> Result<Option<([u8; 64], u32)>> {
    let pass = 2_000u16;
    require!(
        !participant.processed_in_selection_pass(root.recompute_version, pass),
        GiveawayError::InvalidAccount
    );
    participant.mark_selection_pass(root.recompute_version, pass, participant.last_updated_unix);
    if !participant.is_eligible() {
        return Ok(None);
    }
    let key = compute_candidate_key(&root.seed, &participant.wallet);
    if key > root.threshold_key {
        return Ok(None);
    }
    let emission_index = root.record_emitted()?;
    Ok(Some((key, emission_index)))
}

fn finalize_settlement_root(
    giveaway: &mut Account<Giveaway>,
    winners: &mut Account<WinnersLedger>,
    root: &FinalizationLedger,
    now: i64,
) -> Result<()> {
    let winners_count = root.target_winners;
    let pool = giveaway.calculate_winners_pool();
    let per_winner = if winners_count == 0 {
        0
    } else {
        pool / u64::from(winners_count)
    };
    if winners_count > 0 {
        require!(
            per_winner >= MIN_WINNER_PAYOUT_LAMPORTS,
            GiveawayError::InsufficientFunds
        );
    }
    let total = u64::from(winners_count)
        .checked_mul(per_winner)
        .ok_or(GiveawayError::MathOverflow)?;
    let commitment = compute_threshold_commitment(&root.threshold_key, winners_count);
    winners.initialize(
        giveaway.key(),
        root.threshold_key,
        commitment,
        winners_count,
        total,
        per_winner,
        root.recompute_version,
        now,
    );
    winners.reserved[..32].copy_from_slice(&root.seed);
    giveaway.mark_winners_computed()?;
    giveaway.lock_winners();
    winners.lock(now);
    GiveawayEvent::WinnersComputed {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        winners_ledger: winners.key().to_string(),
        merkle_root: hex::encode(commitment),
        seed: hex::encode(root.seed),
        rule_version: GIVEAWAY_RULE_VERSION_V4.to_string(),
        total_eligible: root.eligible_count,
        winners_count,
        total_payout_lamports: total,
        per_winner_lamports: per_winner,
        recompute_version: root.recompute_version,
        winners: vec![],
        timestamp: now,
    }
    .emit();
    GiveawayEvent::WinnersLocked {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        winners_ledger: winners.key().to_string(),
        final_recompute_version: giveaway.recompute_version,
        final_winners_count: winners.winners_count,
        timestamp: now,
    }
    .emit();
    Ok(())
}
