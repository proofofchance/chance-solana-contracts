use crate::state::*;
use anchor_lang::prelude::*;

pub fn hash(parts: &[&[u8]]) -> [u8; 32] {
    // The SBF implementation uses the SHA-256 syscall. Pure Rust SHA-256 made
    // a full 32-key wave commitment exceed the default transaction budget.
    solana_program::hash::hashv(parts).to_bytes()
}
pub fn text_valid(text: &str, max: usize) -> bool {
    !text.is_empty()
        && !text.as_bytes().contains(&0)
        && text.len() <= max
        && text.bytes().any(|b| !b.is_ascii_whitespace())
}
pub fn task_hash(input: &TaskInput) -> Result<[u8; 32]> {
    Ok(hash(&[b"GIVEAWAY_TASK_V2", &input.try_to_vec()?]))
}
pub fn fold(domain: &[u8], previous: &[u8; 32], leaf: &[u8; 32]) -> [u8; 32] {
    hash(&[domain, previous, leaf])
}
pub fn evidence_hash(
    instance: &Pubkey,
    wallet: &Pubkey,
    task: u16,
    version: u8,
    text: &str,
) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_EVIDENCE_V2",
        instance.as_ref(),
        wallet.as_ref(),
        &task.to_le_bytes(),
        &[version],
        &(text.len() as u32).to_le_bytes(),
        text.as_bytes(),
    ])
}
pub fn phrase_commitment(phrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    require!(
        text_valid(phrase, 32) && salt.len() <= 16 && !phrase.as_bytes().contains(&31),
        GiveawayError::Reveal
    );
    Ok(hash(&[phrase.as_bytes(), &[31], salt]))
}
pub fn contribution(wallet: &Pubkey, phrase: &str) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_PHRASE_V2",
        wallet.as_ref(),
        &(phrase.len() as u32).to_le_bytes(),
        phrase.as_bytes(),
    ])
}
pub fn pool_leaf(previous: &[u8; 32], p: &Participant) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_POOL_V2",
        previous,
        &p.index.to_le_bytes(),
        p.wallet.as_ref(),
        &p.contribution,
    ])
}
pub fn seed(instance: &Pubkey, count: u64, aggregate: &[u8; 32], pool: &[u8; 32]) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_SEED_V2",
        instance.as_ref(),
        &count.to_le_bytes(),
        aggregate,
        pool,
    ])
}
pub fn candidate_key(seed: &[u8; 32], wallet: &Pubkey) -> [u8; 64] {
    let score = hash(&[b"GIVEAWAY_RANK_V2", seed, wallet.as_ref()]);
    let mut key = [0; 64];
    key[..32].copy_from_slice(&score);
    key[32..].copy_from_slice(wallet.as_ref());
    key
}
pub fn accepted_root(previous: &[u8; 32], slot: &WinnerSlot) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_ACCEPTED_V2",
        previous,
        &slot.slot.to_le_bytes(),
        &slot.ordinal.to_le_bytes(),
        slot.wallet.as_ref(),
        &slot.evidence_root,
        &slot.revision.to_le_bytes(),
    ])
}
pub fn outcome_hash(g: &Giveaway, instance: &Pubkey) -> [u8; 32] {
    hash(&[
        b"GIVEAWAY_OUTCOME_V2",
        instance.as_ref(),
        &g.rules_hash,
        &g.accepted_root,
        &g.approved_count.to_le_bytes(),
        &g.per_winner.to_le_bytes(),
        &g.fee.to_le_bytes(),
        &g.remainder.to_le_bytes(),
    ])
}
#[allow(clippy::too_many_arguments)]
pub fn creation_hash(
    program: &Pubkey,
    config_key: &Pubkey,
    c: &Config,
    creator: &Pubkey,
    rulebook_key: &Pubkey,
    r: &Rulebook,
    id: u64,
    terms: &Terms,
) -> Result<[u8; 32]> {
    Ok(hash(&[
        b"GIVEAWAY_CREATION_V2",
        program.as_ref(),
        config_key.as_ref(),
        c.registry_program.as_ref(),
        c.registry_config.as_ref(),
        c.authority.as_ref(),
        &c.network_domain,
        &c.fee_bps.to_le_bytes(),
        creator.as_ref(),
        rulebook_key.as_ref(),
        &r.nonce.to_le_bytes(),
        &r.root,
        &r.task_count.to_le_bytes(),
        &id.to_le_bytes(),
        &terms.try_to_vec()?,
    ]))
}
pub fn accounting(funding: u64, bps: u16, winners: u32) -> Result<(u64, u64, u64)> {
    require!(winners > 0 && bps < 10_000, GiveawayError::Funds);
    let fee = ((funding as u128) * (bps as u128) / 10_000) as u64;
    let net = funding.checked_sub(fee).ok_or(GiveawayError::Funds)?;
    let per = net / u64::from(winners);
    require!(per >= 1, GiveawayError::Funds);
    Ok((fee, per, net % u64::from(winners)))
}
/// Keeping the smallest bounded prefix during a complete indexed scan proves
/// exactly the same next wave as sorting the complete unconsumed pool.
pub fn insert_key(keys: &mut Vec<[u8; 64]>, key: [u8; 64]) -> Result<()> {
    match keys.binary_search(&key) {
        Ok(_) => return err!(GiveawayError::Coverage),
        Err(pos) if pos < WAVE_SIZE => {
            keys.insert(pos, key);
            keys.truncate(WAVE_SIZE);
        }
        Err(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn bytes(hex: &str) -> [u8; 32] {
        let mut out = [0; 32];
        for (i, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
            out[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
        }
        out
    }
    fn wallet(n: u8) -> Pubkey {
        let mut b = [0; 32];
        b[31] = n;
        Pubkey::new_from_array(b)
    }
    #[test]
    fn native_encoding_matches_independent_python_golden_vectors() {
        // Pinned external constants from audit/schemas/giveaway-v2.json. These
        // expectations are not derived by the Rust implementation under test.
        let task = TaskInput {
            task_id: 0,
            title: "Follow".into(),
            instructions: "Follow the published brand account.".into(),
            target_uri: "https://example.invalid/brand".into(),
            evidence_prompt: "Public account identifier".into(),
            max_evidence_bytes: 256,
        };
        let th = task_hash(&task).unwrap();
        assert_eq!(
            hex(&th),
            "cec7b5be6ace07b86bdbd648b5acd0eea2993b51db047926c4dfaa45f3d2183d"
        );
        assert_eq!(
            hex(&fold(b"GIVEAWAY_TASKS_V2", &[0; 32], &th)),
            "e00cd874684633e78502c8c663e301c78fe9257bee48374ce4c9f30dea2508cb"
        );
        let mut address = [0; 32];
        address[30] = 3;
        address[31] = 232;
        let instance = Pubkey::new_from_array(address);
        assert_eq!(
            hex(&evidence_hash(&instance, &wallet(1), 0, 1, "proof")),
            "8a96a28f7d777d8a23f1636aec1352c5d539457337d35ee9babfbe622a60f5f9"
        );
        assert_eq!(
            hex(&contribution(&wallet(1), "Chance")),
            "40819fa9b7f586b94abb32bde9767a81cc54e813f7cd17d8e9a0cf558fdbcd08"
        );
        let mut pool = [0; 32];
        let mut aggregate = [0; 32];
        for n in 1..=4u8 {
            let mut p =
                Participant::try_deserialize_unchecked(&mut &vec![0; Participant::SPACE][..])
                    .unwrap();
            p.wallet = wallet(n);
            p.index = u64::from(n - 1);
            p.contribution = contribution(&p.wallet, "Chance");
            pool = pool_leaf(&pool, &p);
            for (a, b) in aggregate.iter_mut().zip(p.contribution) {
                *a ^= b;
            }
        }
        assert_eq!(
            hex(&pool),
            "fe90a4baa5610e784e3ddc507ad48825c32bd5a392f90d05cc732994701124a6"
        );
        let seed = seed(&instance, 4, &aggregate, &pool);
        assert_eq!(
            hex(&seed),
            "9387893eb168ab2ce5b7cba12793c4b4eb4c1998c07098a8c3a79557c4efb465"
        );
        assert_eq!(hex(&candidate_key(&seed,&wallet(2))),"8042265524ca91220f4eea0c083b87f5d0ccb9fd7b4877ded9ca24e6f120273e0000000000000000000000000000000000000000000000000000000000000002");
        let mut root = [0; 32];
        for (slot, n, ordinal, evidence) in [
            (
                0,
                3,
                1,
                "6073f64ca70e874920e8b6c29dad9817440794130f7427986ac24cc0f1b7f540",
            ),
            (
                1,
                4,
                3,
                "31e8e9a476c2d3051471f42da4135f7b26f8732282f05da8167e11eec30b73c8",
            ),
        ] {
            let s = WinnerSlot {
                giveaway: instance,
                slot,
                wallet: wallet(n),
                ordinal,
                revision: 3,
                evidence_root: bytes(evidence),
                approved: true,
                paid: false,
            };
            root = accepted_root(&root, &s);
        }
        assert_eq!(
            hex(&root),
            "f047aac420cf5953b60cedd2407309fa4f877e644b51c269982639332a55dd2f"
        );
        let mut g =
            Giveaway::try_deserialize_unchecked(&mut &vec![0; Giveaway::SPACE][..]).unwrap();
        g.rules_hash = [0x11; 32];
        g.accepted_root = root;
        g.approved_count = 2;
        g.per_winner = 4751;
        g.fee = 500;
        g.remainder = 1;
        assert_eq!(
            hex(&outcome_hash(&g, &instance)),
            "ee3ae9b48cf37d09a75235f3f7e9da1b76fd04464c0a66169126351417433f50"
        );
    }
    #[test]
    fn waves_match_independent_full_sort_for_all_rejections_and_ties() {
        let mut source = Vec::new();
        for n in 0..MAX_REGISTERED {
            let mut key = [0; 64];
            // Deliberate equal score prefixes; wallet bytes break all ties.
            key[..8].copy_from_slice(&(n % 7).to_be_bytes());
            key[56..].copy_from_slice(&n.to_be_bytes());
            source.push(key);
        }
        let mut expected = source.clone();
        expected.sort();
        let mut frontier = None;
        let mut actual = Vec::new();
        loop {
            let mut wave = Vec::new();
            for key in source.iter().rev().copied() {
                if frontier.is_none_or(|f| key > f) {
                    insert_key(&mut wave, key).unwrap();
                }
            }
            if wave.is_empty() {
                break;
            }
            frontier = wave.last().copied();
            actual.extend(wave);
        }
        assert_eq!(actual, expected);
    }
    #[test]
    fn salt_is_only_in_commitment_and_accounting_conserves_funding() {
        let w = Pubkey::new_unique();
        assert_ne!(
            phrase_commitment("same", b"one").unwrap(),
            phrase_commitment("same", b"two").unwrap()
        );
        assert_eq!(contribution(&w, "same"), contribution(&w, "same"));
        for d in [1, 19, 10_003, u64::MAX] {
            for b in [0, 500, 9999] {
                for w in [1, 2, 1000] {
                    if let Ok((f, p, r)) = accounting(d, b, w) {
                        assert_eq!(
                            u128::from(f) + u128::from(p) * u128::from(w) + u128::from(r),
                            u128::from(d)
                        );
                    }
                }
            }
        }
        assert!(accounting(1, 500, 2).is_err());
        assert!(!text_valid(" \t\r\n", 256));
        assert!(!text_valid("", 256));
    }
}
