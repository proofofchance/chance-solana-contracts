use crate::state::*;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = bootstrap, space = Config::SPACE, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    #[account(mut, address = crate::INITIALIZATION_AUTHORITY @ GiveawayError::Unauthorized)]
    pub bootstrap: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct BeginRulebook<'info> {
    #[account(init, payer = creator, space = Rulebook::SPACE, seeds = [b"rules", creator.key().as_ref(), &nonce.to_le_bytes()], bump)]
    pub rulebook: Account<'info, Rulebook>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
#[instruction(input: TaskInput)]
pub struct AppendTask<'info> {
    #[account(mut, has_one = creator, seeds = [b"rules", creator.key().as_ref(), &rulebook.nonce.to_le_bytes()], bump)]
    pub rulebook: Account<'info, Rulebook>,
    #[account(init, payer = creator, space = Task::SPACE, seeds = [b"task", rulebook.key().as_ref(), &input.task_id.to_le_bytes()], bump)]
    pub task: Account<'info, Task>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct RulebookOwner<'info> {
    #[account(mut, has_one = creator, seeds = [b"rules", creator.key().as_ref(), &rulebook.nonce.to_le_bytes()], bump)]
    pub rulebook: Account<'info, Rulebook>,
    pub creator: Signer<'info>,
}
#[derive(Accounts)]
pub struct CloseRulebook<'info> {
    #[account(mut, close = creator, has_one = creator, constraint = rulebook.instance == Pubkey::default() @ GiveawayError::State)]
    pub rulebook: Account<'info, Rulebook>,
    #[account(mut)]
    pub creator: Signer<'info>,
}
#[derive(Accounts)]
#[instruction(id: u64)]
pub struct Create<'info> {
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    #[account(mut, has_one = creator, seeds = [b"rules", creator.key().as_ref(), &rulebook.nonce.to_le_bytes()], bump)]
    pub rulebook: Account<'info, Rulebook>,
    #[account(init, payer = creator, space = Giveaway::SPACE, seeds = [b"giveaway", config.key().as_ref(), &id.to_le_bytes()], bump)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(init, payer = creator, space = Vault::SPACE, seeds = [b"vault", giveaway.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: Registry CPI validates executable and fixed binding.
    pub registry_program: UncheckedAccount<'info>,
    /// CHECK: Registry schema, owner and PDA validated by CPI helper.
    #[account(mut)]
    pub registry_config: UncheckedAccount<'info>,
    /// CHECK: Registry release PDA validated by CPI helper.
    pub release: UncheckedAccount<'info>,
    /// CHECK: Canonical release PDA validated and signed in CPI helper.
    pub release_signer: UncheckedAccount<'info>,
    /// CHECK: Created and checked by registry CPI.
    #[account(mut)]
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: Canonical cross-release nonce checked by registry CPI.
    #[account(mut)]
    pub business_key: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct BeginEntry<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(init, payer = wallet, space = Participant::SPACE, seeds = [b"participant", giveaway.key().as_ref(), wallet.key().as_ref()], bump)]
    pub participant: Account<'info, Participant>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct EntryOwner<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = wallet, has_one = giveaway, seeds = [b"participant", giveaway.key().as_ref(), wallet.key().as_ref()], bump)]
    pub participant: Account<'info, Participant>,
    pub wallet: Signer<'info>,
}
#[derive(Accounts)]
#[instruction(task_id: u16, version: u8)]
pub struct PublishEvidence<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = wallet, has_one = giveaway, seeds = [b"participant", giveaway.key().as_ref(), wallet.key().as_ref()], bump)]
    pub participant: Account<'info, Participant>,
    #[account(constraint = task.rulebook == giveaway.rulebook @ GiveawayError::Account,
        seeds = [b"task", giveaway.rulebook.as_ref(), &task_id.to_le_bytes()], bump)]
    pub task: Account<'info, Task>,
    #[account(init, payer = wallet, space = Evidence::SPACE, seeds = [b"evidence", participant.key().as_ref(), &task_id.to_le_bytes(), &[version]], bump)]
    pub evidence: Account<'info, Evidence>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct BeginSeal<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = wallet, has_one = giveaway)]
    pub participant: Account<'info, Participant>,
    #[account(init_if_needed, payer = wallet, space = SealSession::SPACE, seeds = [b"entrySeal", participant.key().as_ref()], bump)]
    pub session: Account<'info, SealSession>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct ProcessSeal<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = wallet, has_one = giveaway)]
    pub participant: Account<'info, Participant>,
    #[account(mut, has_one = participant, seeds = [b"entrySeal", participant.key().as_ref()], bump)]
    pub session: Account<'info, SealSession>,
    pub wallet: Signer<'info>,
}
#[derive(Accounts)]
pub struct Receipt<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway)]
    pub participant: Account<'info, Participant>,
    /// CHECK: Canonical instructions sysvar address; receipt payload checked exactly.
    #[account(address = solana_program::sysvar::instructions::ID)]
    pub instructions_sysvar: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct Reveal<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway)]
    pub participant: Account<'info, Participant>,
    pub actor: Signer<'info>,
}
#[derive(Accounts)]
pub struct BeginPool<'info> {
    #[account(mut, has_one = host)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(init, payer = host, space = Ranking::SPACE, seeds = [b"ranking", giveaway.key().as_ref()], bump)]
    pub ranking: Box<Account<'info, Ranking>>,
    #[account(mut)]
    pub host: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct RankChunk<'info> {
    #[account(mut, has_one = host)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"ranking", giveaway.key().as_ref()], bump)]
    pub ranking: Box<Account<'info, Ranking>>,
    pub host: Signer<'info>,
}
#[derive(Accounts)]
#[instruction(ordinal: u64)]
pub struct OpenCandidate<'info> {
    #[account(mut, has_one = host)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"ranking", giveaway.key().as_ref()], bump)]
    pub ranking: Box<Account<'info, Ranking>>,
    #[account(has_one = giveaway)]
    pub participant: Account<'info, Participant>,
    #[account(init, payer = host, space = Candidate::SPACE, seeds = [b"candidate", giveaway.key().as_ref(), &ordinal.to_le_bytes()], bump)]
    pub candidate: Account<'info, Candidate>,
    #[account(init_if_needed, payer = host, space = WinnerSlot::SPACE, seeds = [b"winner", giveaway.key().as_ref(), &giveaway.approved_count.to_le_bytes()], bump)]
    pub slot: Account<'info, WinnerSlot>,
    #[account(mut)]
    pub host: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct ReviewCandidate<'info> {
    #[account(mut, has_one = brand)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"candidate", giveaway.key().as_ref(), &candidate.ordinal.to_le_bytes()], bump)]
    pub candidate: Account<'info, Candidate>,
    #[account(mut, seeds = [b"winner", giveaway.key().as_ref(), &candidate.slot.to_le_bytes()], bump)]
    pub slot: Account<'info, WinnerSlot>,
    pub brand: Signer<'info>,
}
#[derive(Accounts)]
pub struct Finalize<'info> {
    #[account(mut, has_one = host)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    pub host: Signer<'info>,
}
#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"vault", giveaway.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    /// CHECK: Immutable creator destination; lamport transfers execute no recipient code.
    #[account(mut, address = giveaway.creator)]
    pub creator: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct PayWinner<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"vault", giveaway.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut, has_one = giveaway, seeds = [b"winner", giveaway.key().as_ref(), &slot.slot.to_le_bytes()], bump)]
    pub slot: Account<'info, WinnerSlot>,
    /// CHECK: Fixed approved wallet, no redirect authority.
    #[account(mut, address = slot.wallet)]
    pub recipient: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct PayBeneficiaries<'info> {
    #[account(mut)]
    pub giveaway: Box<Account<'info, Giveaway>>,
    #[account(mut, has_one = giveaway, seeds = [b"vault", giveaway.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    /// CHECK: Fixed snapshotted fee destination.
    #[account(mut, address = giveaway.host)]
    pub host: UncheckedAccount<'info>,
    /// CHECK: Fixed funding/remainder destination.
    #[account(mut, address = giveaway.creator)]
    pub creator: UncheckedAccount<'info>,
}
