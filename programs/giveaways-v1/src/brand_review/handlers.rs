use crate::{contexts::*, crypto::*, registry, state::*};
use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

fn record(kind: u8, address: Pubkey, data: Vec<u8>) {
    emit!(BrandReviewRecord { kind, address, data });
}

fn audit(
    kind: u8,
    instance: Pubkey,
    actor: Pubkey,
    sequence: u64,
    hash: [u8; 32],
    value: u64,
) -> Result<()> {
    emit!(Audit {
        kind,
        instance,
        actor,
        sequence,
        hash,
        value,
        timestamp: Clock::get()?.unix_timestamp
    });
    Ok(())
}
fn entry_open(g: &Giveaway) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    require!(
        g.status == OPEN && now >= g.terms.entry_start && now < g.terms.entry_end,
        GiveawayError::Time
    );
    Ok(())
}
fn host_open(g: &Giveaway) -> Result<()> {
    require!(
        Clock::get()?.unix_timestamp < g.terms.absolute_deadline,
        GiveawayError::Time
    );
    Ok(())
}
fn mutate(g: &mut Giveaway, p: &mut Participant) -> Result<()> {
    if p.sealed {
        g.sealed_count = g
            .sealed_count
            .checked_sub(1)
            .ok_or(GiveawayError::Coverage)?;
    }
    p.sealed = false;
    p.revision = p.revision.checked_add(1).ok_or(GiveawayError::Limit)?;
    require!(
        p.revision <= (MAX_TASKS as u32) * u32::from(MAX_VERSIONS) + 4,
        GiveawayError::Limit
    );
    Ok(())
}
pub fn initialize(
    ctx: Context<Initialize>,
    host: Pubkey,
    fee_bps: u16,
    network_domain: [u8; 32],
) -> Result<()> {
    require!(
        host != Pubkey::default() && fee_bps < 10_000 && network_domain != [0; 32],
        GiveawayError::Limit
    );
    let c = &mut ctx.accounts.config;
    c.authority = host;
    c.fee_bps = fee_bps;
    c.network_domain = network_domain;
    c.next_giveaway_id = 1;
    record(0, ctx.accounts.config.key(), ctx.accounts.config.try_to_vec()?);

    Ok(())
}
pub fn begin_rulebook(ctx: Context<BeginRulebook>, nonce: u64, task_count: u16) -> Result<()> {
    require!(
        task_count > 0 && usize::from(task_count) <= MAX_TASKS,
        GiveawayError::Limit
    );
    let r = &mut ctx.accounts.rulebook;
    r.creator = ctx.accounts.creator.key();
    r.nonce = nonce;
    r.task_count = task_count;
    record(1, r.key(), r.try_to_vec()?);

    audit(1, r.key(), r.creator, nonce, [0; 32], u64::from(task_count))
}
pub fn append_task(ctx: Context<AppendTask>, input: TaskInput) -> Result<()> {
    let r = &mut ctx.accounts.rulebook;
    require!(
        !r.sealed
            && r.instance == Pubkey::default()
            && input.task_id == r.appended
            && input.task_id < r.task_count,
        GiveawayError::Coverage
    );
    require!(
        text_valid(&input.title, 64)
            && text_valid(&input.instructions, 256)
            && (input.target_uri.is_empty() || text_valid(&input.target_uri, 128))
            && text_valid(&input.evidence_prompt, 128)
            && input.max_evidence_bytes > 0
            && usize::from(input.max_evidence_bytes) <= MAX_EVIDENCE,
        GiveawayError::Limit
    );
    let bytes = input.title.len()
        + input.instructions.len()
        + input.target_uri.len()
        + input.evidence_prompt.len();
    r.total_bytes = r
        .total_bytes
        .checked_add(bytes as u32)
        .ok_or(GiveawayError::Limit)?;
    let h = task_hash(&input)?;
    ctx.accounts.task.rulebook = r.key();
    ctx.accounts.task.input = input;
    ctx.accounts.task.hash = h;
    r.root = fold(b"GIVEAWAY_TASKS_V2", &r.root, &h);
    r.appended += 1;
    record(2, ctx.accounts.task.key(), ctx.accounts.task.try_to_vec()?);
    record(1, r.key(), r.try_to_vec()?);

    audit(
        2,
        r.key(),
        r.creator,
        u64::from(r.appended - 1),
        h,
        bytes as u64,
    )
}
pub fn seal_rulebook(ctx: Context<RulebookOwner>) -> Result<()> {
    let r = &mut ctx.accounts.rulebook;
    require!(
        !r.sealed && r.appended == r.task_count && r.instance == Pubkey::default(),
        GiveawayError::Coverage
    );
    r.sealed = true;
    record(1, r.key(), r.try_to_vec()?);

    audit(
        3,
        r.key(),
        r.creator,
        u64::from(r.task_count),
        r.root,
        u64::from(r.total_bytes),
    )
}
pub fn close_rulebook(ctx: Context<CloseRulebook>) -> Result<()> {
    // Published task accounts remain immutable; closing the unbound header is
    // allowed only before any task is published, preventing namespace reuse.
    require!(ctx.accounts.rulebook.appended == 0, GiveawayError::State);
    Ok(())
}
pub fn create(ctx: Context<Create>, id: u64, terms: Terms, expected: [u8; 32]) -> Result<()> {
    let c = &mut ctx.accounts.config;
    let r = &mut ctx.accounts.rulebook;
    require!(
        id == c.next_giveaway_id && r.sealed && r.instance == Pubkey::default(),
        GiveawayError::State
    );
    let now = Clock::get()?.unix_timestamp;
    require!(
        terms.winners > 0
            && terms.winners <= MAX_WINNERS
            && terms.entry_start >= now
            && terms.entry_end > terms.entry_start
            && terms.upload_end > terms.entry_end
            && terms.remediation_end > terms.upload_end
            && terms.opening_cutoff > terms.remediation_end
            && terms.review_seconds > 0
            && terms.finalization_reserve_seconds > 0,
        GiveawayError::Time
    );
    let needed = terms
        .opening_cutoff
        .checked_add(i64::from(terms.review_seconds))
        .and_then(|x| x.checked_add(i64::from(terms.finalization_reserve_seconds)))
        .ok_or(GiveawayError::Time)?;
    require!(
        needed <= terms.absolute_deadline
            && terms
                .absolute_deadline
                .checked_sub(now)
                .ok_or(GiveawayError::Time)?
                <= 90 * 24 * 3600,
        GiveawayError::Time
    );
    accounting(terms.funding, c.fee_bps, terms.winners)?;
    let rules = creation_hash(
        ctx.program_id,
        &c.key(),
        c,
        &ctx.accounts.creator.key(),
        &r.key(),
        r,
        id,
        &terms,
    )?;
    require!(rules == expected, GiveawayError::CreationMismatch);
    let g = &mut ctx.accounts.giveaway;
    g.id = id;
    g.config = c.key();
    g.rulebook = r.key();
    g.creator = ctx.accounts.creator.key();
    g.brand = g.creator;
    g.host = c.authority;
    g.vault = ctx.accounts.vault.key();
    g.rules_hash = rules;
    g.network_domain = c.network_domain;
    g.task_root = r.root;
    g.task_count = r.task_count;
    g.fee_bps = c.fee_bps;
    g.terms = terms;
    ctx.accounts.vault.giveaway = g.key();
    transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.creator.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        g.terms.funding,
    )?;
    registry::record(
        ctx.program_id,
        c,
        &g.to_account_info(),
        &ctx.accounts.creator.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.registry_program.to_account_info(),
        &ctx.accounts.registry_config.to_account_info(),
        &ctx.accounts.release.to_account_info(),
        &ctx.accounts.release_signer.to_account_info(),
        &ctx.accounts.registry_entry.to_account_info(),
        &ctx.accounts.business_key.to_account_info(),
        g,
        r.nonce,
    )?;
    r.instance = g.key();
    c.next_giveaway_id = c
        .next_giveaway_id
        .checked_add(1)
        .ok_or(GiveawayError::Limit)?;
    record(3, g.key(), g.try_to_vec()?);
    record(1, r.key(), r.try_to_vec()?);

    audit(4, g.key(), g.creator, id, rules, g.terms.funding)
}
pub fn begin_entry(ctx: Context<BeginEntry>, commitment: [u8; 32]) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    entry_open(g)?;
    require!(commitment != [0; 32], GiveawayError::Limit);
    let p = &mut ctx.accounts.participant;
    p.giveaway = g.key();
    p.wallet = ctx.accounts.wallet.key();
    p.index = u64::MAX;
    p.commitment = commitment;
    p.revision = 1;
    p.bump = ctx.bumps.participant;
    record(4, p.key(), p.try_to_vec()?);

    audit(5, g.key(), p.wallet, p.index, commitment, 1)
}
pub fn revise_commitment(ctx: Context<EntryOwner>, commitment: [u8; 32]) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    entry_open(g)?;
    require!(commitment != [0; 32], GiveawayError::Limit);
    let p = &mut ctx.accounts.participant;
    mutate(g, p)?;
    p.commitment = commitment;
    record(4, p.key(), p.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        6,
        g.key(),
        p.wallet,
        p.index,
        commitment,
        u64::from(p.revision),
    )
}
pub fn publish_evidence(
    ctx: Context<PublishEvidence>,
    task_id: u16,
    version: u8,
    text: String,
) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    entry_open(g)?;
    require!(
        task_id < g.task_count
            && text_valid(
                &text,
                usize::from(ctx.accounts.task.input.max_evidence_bytes)
            ),
        GiveawayError::Limit
    );
    let p = &mut ctx.accounts.participant;
    let index = usize::from(task_id);
    require!(
        version > 0 && version <= MAX_VERSIONS && version == p.versions[index] + 1,
        GiveawayError::Limit
    );
    mutate(g, p)?;
    p.current_bytes = p
        .current_bytes
        .checked_sub(u32::from(p.lengths[index]))
        .and_then(|x| x.checked_add(text.len() as u32))
        .ok_or(GiveawayError::Limit)?;
    require!(p.current_bytes <= 8_192, GiveawayError::Limit);
    p.versions[index] = version;
    p.lengths[index] = text.len() as u16;
    let e = &mut ctx.accounts.evidence;
    e.participant = p.key();
    e.task_id = task_id;
    e.version = version;
    e.hash = evidence_hash(&g.key(), &p.wallet, task_id, version, &text);
    e.text = text;
    e.published_at = Clock::get()?.unix_timestamp;
    e.bump = ctx.bumps.evidence;
    record(5, e.key(), e.try_to_vec()?);
    record(4, p.key(), p.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        7,
        g.key(),
        p.wallet,
        u64::from(task_id),
        e.hash,
        u64::from(version),
    )
}
pub fn begin_seal(ctx: Context<BeginSeal>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    entry_open(g)?;
    let p = &mut ctx.accounts.participant;
    require!(!p.sealed, GiveawayError::State);
    let s = &mut ctx.accounts.session;
    s.participant = p.key();
    s.revision = p.revision;
    s.cursor = 0;
    s.root = [0; 32];
    s.total_bytes = 0;
    s.active = true;
    record(6, s.key(), s.try_to_vec()?);

    Ok(())
}
pub fn process_seal(ctx: Context<ProcessSeal>) -> Result<()> {
    let g = &ctx.accounts.giveaway;
    entry_open(g)?;
    let p = &ctx.accounts.participant;
    let s = &mut ctx.accounts.session;
    require!(
        s.active
            && s.revision == p.revision
            && !ctx.remaining_accounts.is_empty()
            && ctx.remaining_accounts.len() <= 8,
        GiveawayError::Coverage
    );
    for info in ctx.remaining_accounts {
        require!(
            s.cursor < g.task_count && info.owner == ctx.program_id,
            GiveawayError::Coverage
        );
        let version = p.versions[usize::from(s.cursor)];
        require!(version > 0, GiveawayError::Coverage);
        let e = Evidence::try_deserialize(&mut &info.try_borrow_data()?[..])?;
        let expected = Pubkey::create_program_address(
            &[
                b"evidence",
                p.key().as_ref(),
                &s.cursor.to_le_bytes(),
                &[version],
                &[e.bump],
            ],
            ctx.program_id,
        )
        .map_err(|_| error!(GiveawayError::Account))?;
        require_keys_eq!(*info.key, expected, GiveawayError::Account);
        require!(
            e.participant == p.key()
                && e.task_id == s.cursor
                && e.version == version
                && e.text.len() == usize::from(p.lengths[usize::from(s.cursor)]),
            GiveawayError::Coverage
        );
        s.root = fold(b"GIVEAWAY_ENTRY_V2", &s.root, &e.hash);
        s.total_bytes = s
            .total_bytes
            .checked_add(e.text.len() as u32)
            .ok_or(GiveawayError::Limit)?;
        s.cursor += 1;
    }
    record(6, s.key(), s.try_to_vec()?);

    Ok(())
}
pub fn finish_seal(ctx: Context<ProcessSeal>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    entry_open(g)?;
    let p = &mut ctx.accounts.participant;
    let s = &mut ctx.accounts.session;
    require!(
        s.active
            && !p.sealed
            && s.revision == p.revision
            && s.cursor == g.task_count
            && s.total_bytes == p.current_bytes,
        GiveawayError::Coverage
    );
    if p.index == u64::MAX {
        require!(g.registered_count < MAX_REGISTERED, GiveawayError::Limit);
        p.index = g.registered_count;
        g.registered_count += 1;
    }
    p.sealed = true;
    p.sealed_revision = p.revision;
    p.evidence_root = s.root;
    s.active = false;
    g.sealed_count = g.sealed_count.checked_add(1).ok_or(GiveawayError::Limit)?;
    record(4, p.key(), p.try_to_vec()?);
    record(6, s.key(), s.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        8,
        g.key(),
        p.wallet,
        p.index,
        p.evidence_root,
        u64::from(p.sealed_revision),
    )
}
pub fn receipt_message(g: &Pubkey, network_domain: &[u8; 32], p: &Participant) -> Vec<u8> {
    [
        b"GIVEAWAY_RECEIPT_V2".as_slice(),
        crate::ID.as_ref(),
        network_domain,
        g.as_ref(),
        p.wallet.as_ref(),
        &p.commitment,
        &p.sealed_revision.to_le_bytes(),
        &p.evidence_root,
    ]
    .concat()
}
pub fn attest_receipt(ctx: Context<Receipt>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    let p = &mut ctx.accounts.participant;
    let now = Clock::get()?.unix_timestamp;
    require!(
        g.status == OPEN
            && now >= g.terms.entry_end
            && now < g.terms.upload_end
            && p.sealed
            && !p.attested,
        GiveawayError::Time
    );
    let expected = receipt_message(&g.key(), &g.network_domain, p);
    let ix = solana_program::sysvar::instructions::get_instruction_relative(
        -1,
        &ctx.accounts.instructions_sysvar.to_account_info(),
    )?;
    require!(
        ix.program_id == solana_program::ed25519_program::ID
            && receipt_payload(&ix.data, &g.host, &expected),
        GiveawayError::Reveal
    );
    p.attested = true;
    g.attested_count = g
        .attested_count
        .checked_add(1)
        .ok_or(GiveawayError::Limit)?;
    record(4, p.key(), p.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        9,
        g.key(),
        p.wallet,
        p.index,
        p.evidence_root,
        g.attested_count,
    )
}
pub fn receipt_payload(data: &[u8], host: &Pubkey, message: &[u8]) -> bool {
    if data.len() < 16 || data[0] != 1 || data[1] != 0 {
        return false;
    }
    let read = |i| u16::from_le_bytes([data[i], data[i + 1]]);
    if read(4) != u16::MAX || read(8) != u16::MAX || read(14) != u16::MAX {
        return false;
    }
    let sig = usize::from(read(2));
    let key = usize::from(read(6));
    let msg = usize::from(read(10));
    let len = usize::from(read(12));
    data.get(sig..sig + 64).is_some()
        && data.get(key..key + 32) == Some(host.as_ref())
        && data.get(msg..msg + len) == Some(message)
}
fn publication_allowed(g: &Giveaway, now: i64) -> bool {
    now >= g.terms.upload_end || (g.sealed_count > 0 && g.attested_count == g.sealed_count)
}
pub fn reveal(ctx: Context<Reveal>, phrase: String, salt: Vec<u8>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    let p = &mut ctx.accounts.participant;
    let now = Clock::get()?.unix_timestamp;
    require!(
        g.status == OPEN
            && p.sealed
            && !p.revealed
            && now >= g.terms.entry_end
            && now < g.terms.remediation_end,
        GiveawayError::Time
    );
    if now < g.terms.upload_end {
        require!(
            ctx.accounts.actor.key() == p.wallet
                || (ctx.accounts.actor.key() == g.host
                    && p.attested
                    && publication_allowed(g, now)),
            GiveawayError::Unauthorized
        );
    } else {
        require!(p.attested, GiveawayError::Reveal);
    }
    require!(
        phrase_commitment(&phrase, &salt)? == p.commitment,
        GiveawayError::Reveal
    );
    if !p.attested {
        p.attested = true;
        g.attested_count += 1;
    }
    p.revealed = true;
    p.contribution = contribution(&p.wallet, &phrase);
    g.included_count += 1;
    for (out, part) in g.aggregate.iter_mut().zip(p.contribution) {
        *out ^= part;
    }
    record(4, p.key(), p.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        10,
        g.key(),
        ctx.accounts.actor.key(),
        p.index,
        p.contribution,
        g.included_count,
    )
}
pub fn begin_pool(ctx: Context<BeginPool>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    host_open(g)?;
    let now = Clock::get()?.unix_timestamp;
    require!(
        g.status == OPEN
            && now >= g.terms.entry_end
            && now >= g.terms.upload_end
            && g.attested_count > 0
            && g.included_count == g.attested_count,
        GiveawayError::State
    );
    g.status = POOL_SCAN;
    let r = &mut ctx.accounts.ranking;
    r.giveaway = g.key();
    r.phase = BUILDING;
    record(7, ctx.accounts.ranking.key(), ctx.accounts.ranking.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        11,
        g.key(),
        g.host,
        g.registered_count,
        g.aggregate,
        g.included_count,
    )
}
fn load_participant(
    info: &AccountInfo,
    program: &Pubkey,
    instance: &Pubkey,
    index: u64,
) -> Result<Participant> {
    require!(info.owner == program, GiveawayError::Account);
    let p = Participant::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    let expected = Pubkey::create_program_address(
        &[
            b"participant",
            instance.as_ref(),
            p.wallet.as_ref(),
            &[p.bump],
        ],
        program,
    )
    .map_err(|_| error!(GiveawayError::Account))?;
    require!(
        p.giveaway == *instance && p.index == index && info.key == &expected,
        GiveawayError::Coverage
    );
    Ok(p)
}
pub fn process_pool(ctx: Context<RankChunk>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    host_open(g)?;
    let r = &mut ctx.accounts.ranking;
    require!(
        g.status == POOL_SCAN
            && ctx.remaining_accounts.len() <= 4
            && (!ctx.remaining_accounts.is_empty() || g.registered_count == 0),
        GiveawayError::State
    );
    for info in ctx.remaining_accounts {
        require!(r.cursor < g.registered_count, GiveawayError::Coverage);
        let p = load_participant(info, ctx.program_id, &g.key(), r.cursor)?;
        if p.sealed && p.revealed {
            require!(p.attested, GiveawayError::Coverage);
            g.base_pool_count += 1;
            g.pool_hash = pool_leaf(&g.pool_hash, &p);
        }
        r.cursor += 1;
    }
    if r.cursor == g.registered_count {
        require!(
            g.base_pool_count == g.included_count,
            GiveawayError::Coverage
        );
        g.seed = seed(&g.key(), g.base_pool_count, &g.aggregate, &g.pool_hash);
        g.status = REVIEW;
        r.cursor = 0;
        r.keys.clear();
        r.remaining = 0;
        r.phase = BUILDING;
        audit(
            12,
            g.key(),
            g.host,
            g.base_pool_count,
            g.seed,
            g.registered_count,
        )?;
    }
    record(7, r.key(), r.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    Ok(())
}
pub fn process_wave(ctx: Context<RankChunk>) -> Result<()> {
    let g = &ctx.accounts.giveaway;
    host_open(g)?;
    let r = &mut ctx.accounts.ranking;
    require!(
        g.status == REVIEW
            && !g.pending
            && g.approved_count < g.terms.winners
            && r.phase == BUILDING
            && ctx.remaining_accounts.len() <= 4
            && (!ctx.remaining_accounts.is_empty() || g.registered_count == 0),
        GiveawayError::State
    );
    for info in ctx.remaining_accounts {
        require!(r.cursor < g.registered_count, GiveawayError::Coverage);
        let p = load_participant(info, ctx.program_id, &g.key(), r.cursor)?;
        if p.sealed && p.revealed {
            let key = candidate_key(&g.seed, &p.wallet);
            if !r.has_frontier || key > r.frontier {
                r.remaining += 1;
                insert_key(&mut r.keys, key)?;
            }
        }
        r.cursor += 1;
    }
    if r.cursor == g.registered_count {
        require!(
            r.remaining
                == g.base_pool_count
                    .checked_sub(g.consumed_count)
                    .ok_or(GiveawayError::Coverage)?
                && r.keys.len() == (r.remaining as usize).min(WAVE_SIZE),
            GiveawayError::Coverage
        );
        r.phase = READY;
        r.next_key = 0;
        audit(
            13,
            g.key(),
            g.host,
            u64::from(r.wave),
            hash(&[b"GIVEAWAY_WAVE_V2", &r.keys.try_to_vec()?]),
            r.keys.len() as u64,
        )?;
    }
    record(7, r.key(), r.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    Ok(())
}
pub fn next_wave(ctx: Context<RankChunk>) -> Result<()> {
    let g = &ctx.accounts.giveaway;
    host_open(g)?;
    let r = &mut ctx.accounts.ranking;
    require!(
        g.status == REVIEW
            && !g.pending
            && g.approved_count < g.terms.winners
            && g.consumed_count < g.base_pool_count
            && r.phase == READY
            && usize::from(r.next_key) == r.keys.len(),
        GiveawayError::State
    );
    r.frontier = *r.keys.last().ok_or(GiveawayError::Coverage)?;
    r.has_frontier = true;
    r.wave = r.wave.checked_add(1).ok_or(GiveawayError::Limit)?;
    r.keys.clear();
    r.cursor = 0;
    r.next_key = 0;
    r.remaining = 0;
    r.phase = BUILDING;
    record(7, r.key(), r.try_to_vec()?);

    Ok(())
}
pub fn open_candidate(ctx: Context<OpenCandidate>, ordinal: u64) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    let now = Clock::get()?.unix_timestamp;
    let r = &mut ctx.accounts.ranking;
    let p = &ctx.accounts.participant;
    require!(
        g.status == REVIEW
            && !g.pending
            && g.approved_count < g.terms.winners
            && ordinal == g.consumed_count
            && ordinal < g.base_pool_count
            && now < g.terms.opening_cutoff
            && r.phase == READY,
        GiveawayError::State
    );
    let key = *r
        .keys
        .get(usize::from(r.next_key))
        .ok_or(GiveawayError::Coverage)?;
    require!(
        p.sealed
            && p.revealed
            && key == candidate_key(&g.seed, &p.wallet)
            && !ctx.accounts.slot.approved,
        GiveawayError::Coverage
    );
    let deadline = now
        .checked_add(i64::from(g.terms.review_seconds))
        .ok_or(GiveawayError::Time)?;
    require!(
        deadline
            .checked_add(i64::from(g.terms.finalization_reserve_seconds))
            .ok_or(GiveawayError::Time)?
            <= g.terms.absolute_deadline,
        GiveawayError::Time
    );
    let c = &mut ctx.accounts.candidate;
    c.giveaway = g.key();
    c.ordinal = ordinal;
    c.key = key;
    c.wallet = p.wallet;
    c.entry_revision = p.sealed_revision;
    c.evidence_root = p.evidence_root;
    c.slot = g.approved_count;
    c.opened_at = now;
    c.deadline = deadline;
    g.pending = true;
    g.pending_deadline = deadline;
    r.next_key += 1;
    record(8, c.key(), c.try_to_vec()?);
    record(7, r.key(), r.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        14,
        g.key(),
        g.host,
        ordinal,
        hash(&[&key, &p.evidence_root]),
        u64::from(c.slot),
    )
}
pub fn review_candidate(
    ctx: Context<ReviewCandidate>,
    approve: bool,
    reason: u8,
    note: String,
    task_mask: u32,
) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    let c = &mut ctx.accounts.candidate;
    let now = Clock::get()?.unix_timestamp;
    require!(
        g.status == REVIEW
            && g.pending
            && c.ordinal == g.consumed_count
            && c.verdict == 0
            && c.slot == g.approved_count
            && now < c.deadline
            && now < g.terms.absolute_deadline,
        GiveawayError::State
    );
    require!(
        note.len() <= 128
            && (if approve {
                reason == 0
            } else {
                (1..=3).contains(&reason)
            })
            && (g.task_count == 32 || task_mask < (1u32 << g.task_count)),
        GiveawayError::Limit
    );
    c.verdict = if approve { 1 } else { 2 };
    c.reason = reason;
    c.note = note;
    c.task_mask = task_mask;
    c.decided_at = now;
    if approve {
        let s = &mut ctx.accounts.slot;
        require!(!s.approved, GiveawayError::State);
        s.giveaway = g.key();
        s.slot = g.approved_count;
        s.wallet = c.wallet;
        s.ordinal = c.ordinal;
        s.revision = c.entry_revision;
        s.evidence_root = c.evidence_root;
        s.approved = true;
        g.accepted_root = accepted_root(&g.accepted_root, s);
        g.approved_count += 1;
    }
    g.consumed_count += 1;
    g.pending = false;
    g.pending_deadline = 0;
    record(8, c.key(), c.try_to_vec()?);
    record(9, ctx.accounts.slot.key(), ctx.accounts.slot.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        15,
        g.key(),
        g.brand,
        c.ordinal,
        hash(&[b"GIVEAWAY_DECISION_V2", &c.try_to_vec()?]),
        u64::from(c.verdict),
    )
}
pub fn commit_outcome(ctx: Context<Finalize>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    host_open(g)?;
    require!(
        g.status == REVIEW
            && !g.pending
            && g.approved_count > 0
            && (g.approved_count == g.terms.winners || g.consumed_count == g.base_pool_count),
        GiveawayError::State
    );
    let (f, p, r) = accounting(g.terms.funding, g.fee_bps, g.approved_count)?;
    g.fee = f;
    g.per_winner = p;
    g.remainder = r;
    g.outcome_hash = outcome_hash(g, &g.key());
    g.status = VESTED;
    record(3, g.key(), g.try_to_vec()?);

    audit(
        16,
        g.key(),
        g.host,
        u64::from(g.approved_count),
        g.outcome_hash,
        g.per_winner,
    )
}
pub fn refund_eligible(g: &Giveaway, now: i64) -> bool {
    if g.status == VESTED || g.status == REFUNDED {
        return false;
    }
    now >= g.terms.absolute_deadline
        || (g.pending && now >= g.pending_deadline)
        || (!g.pending
            && (g.status == OPEN
                || g.status == POOL_SCAN
                || (g.status == REVIEW
                    && g.approved_count < g.terms.winners
                    && g.consumed_count < g.base_pool_count))
            && now >= g.terms.opening_cutoff)
        || (g.status == OPEN && now >= g.terms.upload_end && g.attested_count == 0)
        || (g.status == OPEN
            && now >= g.terms.remediation_end
            && g.included_count < g.attested_count)
        || (g.status == REVIEW
            && !g.pending
            && g.consumed_count == g.base_pool_count
            && g.approved_count == 0)
}
fn send(vault: &AccountInfo, recipient: &AccountInfo, amount: u64) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    require!(vault.key != recipient.key, GiveawayError::Account);
    let floor = Rent::get()?.minimum_balance(vault.data_len());
    require!(
        vault
            .lamports()
            .checked_sub(amount)
            .is_some_and(|left| left >= floor),
        GiveawayError::Funds
    );
    let dest = recipient
        .lamports()
        .checked_add(amount)
        .ok_or(GiveawayError::Funds)?;
    **vault.try_borrow_mut_lamports()? -= amount;
    **recipient.try_borrow_mut_lamports()? = dest;
    Ok(())
}
pub fn finalize_refund(ctx: Context<Refund>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    if g.status == REFUNDED {
        return Ok(());
    }
    require!(
        refund_eligible(g, Clock::get()?.unix_timestamp),
        GiveawayError::State
    );
    g.status = REFUNDED;
    send(
        &ctx.accounts.vault.to_account_info(),
        &ctx.accounts.creator.to_account_info(),
        g.terms.funding,
    )?;
    record(3, g.key(), g.try_to_vec()?);

    audit(
        17,
        g.key(),
        g.creator,
        g.consumed_count,
        g.accepted_root,
        g.terms.funding,
    )
}
pub fn pay_winner(ctx: Context<PayWinner>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    let s = &mut ctx.accounts.slot;
    require!(
        g.status == VESTED && s.approved && s.slot < g.approved_count,
        GiveawayError::State
    );
    if s.paid {
        return Ok(());
    }
    s.paid = true;
    g.paid_winners = g.paid_winners.checked_add(1).ok_or(GiveawayError::Funds)?;
    require!(g.paid_winners <= g.approved_count, GiveawayError::Funds);
    send(
        &ctx.accounts.vault.to_account_info(),
        &ctx.accounts.recipient.to_account_info(),
        g.per_winner,
    )?;
    record(9, s.key(), s.try_to_vec()?);
    record(3, g.key(), g.try_to_vec()?);

    audit(
        18,
        g.key(),
        s.wallet,
        u64::from(s.slot),
        g.outcome_hash,
        g.per_winner,
    )
}
pub fn pay_beneficiaries(ctx: Context<PayBeneficiaries>) -> Result<()> {
    let g = &mut ctx.accounts.giveaway;
    require!(g.status == VESTED, GiveawayError::State);
    if !g.fee_paid {
        g.fee_paid = true;
        send(
            &ctx.accounts.vault.to_account_info(),
            &ctx.accounts.host.to_account_info(),
            g.fee,
        )?;
    }
    if !g.remainder_paid {
        g.remainder_paid = true;
        send(
            &ctx.accounts.vault.to_account_info(),
            &ctx.accounts.creator.to_account_info(),
            g.remainder,
        )?;
    }
    record(3, g.key(), g.try_to_vec()?);

    audit(19, g.key(), g.host, 0, g.outcome_hash, g.fee)
}
pub fn withdraw_surplus(ctx: Context<Refund>) -> Result<()> {
    let g = &ctx.accounts.giveaway;
    require!(
        g.status == REFUNDED
            || (g.status == VESTED
                && g.paid_winners == g.approved_count
                && g.fee_paid
                && g.remainder_paid),
        GiveawayError::State
    );
    let v = ctx.accounts.vault.to_account_info();
    let floor = Rent::get()?.minimum_balance(v.data_len());
    let surplus = v
        .lamports()
        .checked_sub(floor)
        .ok_or(GiveawayError::Funds)?;
    send(&v, &ctx.accounts.creator.to_account_info(), surplus)?;
    record(3, g.key(), g.try_to_vec()?);

    audit(20, g.key(), g.creator, 0, g.outcome_hash, surplus)
}
