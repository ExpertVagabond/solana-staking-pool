use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};
use solana_dao_voting::cpi as dao_cpi;
use solana_dao_voting::cpi::accounts::CastVote as DaoCastVote;
use solana_dao_voting::program::SolanaDaoVoting;
use solana_dao_voting::{Dao, Proposal};

declare_id!("E3xDfoQKgqdCNgnK1B77xgRjKYjEsBCXvPxZDGoxercH");

const PRECISION: u128 = 1_000_000_000_000_000_000;

#[program]
pub mod solana_staking_pool {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>, reward_rate: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.authority = ctx.accounts.authority.key();
        pool.stake_mint = ctx.accounts.stake_mint.key();
        pool.reward_mint = ctx.accounts.reward_mint.key();
        pool.total_staked = 0;
        pool.reward_rate = reward_rate;
        pool.last_update_ts = Clock::get()?.unix_timestamp;
        pool.accumulated_reward_per_token = 0;
        pool.bump = ctx.bumps.pool;

        emit!(PoolInitialized {
            authority: pool.authority,
            staking_mint: pool.stake_mint,
            reward_mint: pool.reward_mint,
            reward_rate,
        });
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        require!(amount > 0, StakingError::ZeroAmount);
        update_rewards(&mut ctx.accounts.pool)?;

        let pool = &ctx.accounts.pool;
        let entry = &mut ctx.accounts.stake_entry;

        if entry.amount > 0 {
            let pending = calc_pending(
                entry.amount,
                pool.accumulated_reward_per_token,
                entry.reward_debt,
            )?;
            if pending > 0 {
                let seeds: &[&[u8]] = &[
                    b"pool",
                    pool.authority.as_ref(),
                    pool.stake_mint.as_ref(),
                    &[pool.bump],
                ];
                transfer_checked(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        TransferChecked {
                            from: ctx.accounts.reward_vault.to_account_info(),
                            to: ctx.accounts.user_reward_ata.to_account_info(),
                            authority: ctx.accounts.pool.to_account_info(),
                            mint: ctx.accounts.reward_mint.to_account_info(),
                        },
                        &[seeds],
                    ),
                    pending,
                    ctx.accounts.reward_mint.decimals,
                )?;
            }
        }

        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.user_stake_ata.to_account_info(),
                    to: ctx.accounts.stake_vault.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                    mint: ctx.accounts.stake_mint.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.stake_mint.decimals,
        )?;

        let pool = &mut ctx.accounts.pool;
        let pool_key = pool.key();
        pool.total_staked = pool
            .total_staked
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        let acc_reward = pool.accumulated_reward_per_token;
        let entry = &mut ctx.accounts.stake_entry;
        entry.owner = ctx.accounts.owner.key();
        entry.pool = pool_key;
        entry.amount = entry
            .amount
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        entry.reward_debt = calc_debt(entry.amount, acc_reward)?;
        entry.bump = ctx.bumps.stake_entry;

        emit!(TokensStaked {
            pool: pool_key,
            user: ctx.accounts.owner.key(),
            amount,
        });
        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        require!(amount > 0, StakingError::ZeroAmount);
        require!(
            ctx.accounts.stake_entry.amount >= amount,
            StakingError::InsufficientStake
        );
        update_rewards(&mut ctx.accounts.pool)?;

        let pool = &ctx.accounts.pool;
        let entry = &mut ctx.accounts.stake_entry;
        let pending = calc_pending(
            entry.amount,
            pool.accumulated_reward_per_token,
            entry.reward_debt,
        )?;
        let seeds: &[&[u8]] = &[
            b"pool",
            pool.authority.as_ref(),
            pool.stake_mint.as_ref(),
            &[pool.bump],
        ];

        if pending > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.reward_vault.to_account_info(),
                        to: ctx.accounts.user_reward_ata.to_account_info(),
                        authority: ctx.accounts.pool.to_account_info(),
                        mint: ctx.accounts.reward_mint.to_account_info(),
                    },
                    &[seeds],
                ),
                pending,
                ctx.accounts.reward_mint.decimals,
            )?;
        }

        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.stake_vault.to_account_info(),
                    to: ctx.accounts.user_stake_ata.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                    mint: ctx.accounts.stake_mint.to_account_info(),
                },
                &[seeds],
            ),
            amount,
            ctx.accounts.stake_mint.decimals,
        )?;

        let pool = &mut ctx.accounts.pool;
        pool.total_staked = pool
            .total_staked
            .checked_sub(amount)
            .ok_or(StakingError::MathOverflow)?;
        let entry = &mut ctx.accounts.stake_entry;
        entry.amount = entry
            .amount
            .checked_sub(amount)
            .ok_or(StakingError::MathOverflow)?;
        entry.reward_debt = calc_debt(entry.amount, pool.accumulated_reward_per_token)?;

        emit!(TokensUnstaked {
            pool: pool.key(),
            user: ctx.accounts.owner.key(),
            amount,
        });
        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        update_rewards(&mut ctx.accounts.pool)?;
        let pool = &ctx.accounts.pool;
        let entry = &mut ctx.accounts.stake_entry;
        let pending = calc_pending(
            entry.amount,
            pool.accumulated_reward_per_token,
            entry.reward_debt,
        )?;
        require!(pending > 0, StakingError::NoPendingRewards);

        let seeds: &[&[u8]] = &[
            b"pool",
            pool.authority.as_ref(),
            pool.stake_mint.as_ref(),
            &[pool.bump],
        ];
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.reward_vault.to_account_info(),
                    to: ctx.accounts.user_reward_ata.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                    mint: ctx.accounts.reward_mint.to_account_info(),
                },
                &[seeds],
            ),
            pending,
            ctx.accounts.reward_mint.decimals,
        )?;

        entry.reward_debt = calc_debt(entry.amount, pool.accumulated_reward_per_token)?;

        emit!(RewardsClaimed {
            pool: pool.key(),
            user: ctx.accounts.owner.key(),
            amount: pending,
        });
        Ok(())
    }

    pub fn update_reward_rate(ctx: Context<UpdateRewardRate>, new_rate: u64) -> Result<()> {
        update_rewards(&mut ctx.accounts.pool)?;
        ctx.accounts.pool.reward_rate = new_rate;
        Ok(())
    }

    /// Composite instruction: stake tokens in the pool, then CPI into DAO voting
    /// to cast a weighted vote on a proposal using the staked amount.
    pub fn stake_and_vote(ctx: Context<StakeAndVote>, amount: u64, side: bool) -> Result<()> {
        require!(amount > 0, StakingError::ZeroAmount);
        update_rewards(&mut ctx.accounts.pool)?;

        // --- Stake logic (mirrors `stake`) ---
        let pool = &ctx.accounts.pool;
        let entry = &mut ctx.accounts.stake_entry;

        if entry.amount > 0 {
            let pending = calc_pending(
                entry.amount,
                pool.accumulated_reward_per_token,
                entry.reward_debt,
            )?;
            if pending > 0 {
                let seeds: &[&[u8]] = &[
                    b"pool",
                    pool.authority.as_ref(),
                    pool.stake_mint.as_ref(),
                    &[pool.bump],
                ];
                transfer_checked(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        TransferChecked {
                            from: ctx.accounts.reward_vault.to_account_info(),
                            to: ctx.accounts.user_reward_ata.to_account_info(),
                            authority: ctx.accounts.pool.to_account_info(),
                            mint: ctx.accounts.reward_mint.to_account_info(),
                        },
                        &[seeds],
                    ),
                    pending,
                    ctx.accounts.reward_mint.decimals,
                )?;
            }
        }

        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.user_stake_ata.to_account_info(),
                    to: ctx.accounts.stake_vault.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                    mint: ctx.accounts.stake_mint.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.stake_mint.decimals,
        )?;

        let pool = &mut ctx.accounts.pool;
        let pool_key = pool.key();
        pool.total_staked = pool
            .total_staked
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        let acc_reward = pool.accumulated_reward_per_token;
        let entry = &mut ctx.accounts.stake_entry;
        entry.owner = ctx.accounts.owner.key();
        entry.pool = pool_key;
        entry.amount = entry
            .amount
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        entry.reward_debt = calc_debt(entry.amount, acc_reward)?;
        entry.bump = ctx.bumps.stake_entry;

        emit!(TokensStaked {
            pool: pool_key,
            user: ctx.accounts.owner.key(),
            amount,
        });

        // --- CPI into DAO voting: cast_vote ---
        let cpi_accounts = DaoCastVote {
            voter: ctx.accounts.owner.to_account_info(),
            dao: ctx.accounts.dao.to_account_info(),
            proposal: ctx.accounts.proposal.to_account_info(),
            vote_record: ctx.accounts.vote_record.to_account_info(),
            governance_mint: ctx.accounts.governance_mint.to_account_info(),
            voter_token_account: ctx.accounts.voter_governance_ata.to_account_info(),
            vote_vault: ctx.accounts.dao_vote_vault.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.dao_voting_program.to_account_info(),
            cpi_accounts,
        );
        dao_cpi::cast_vote(cpi_ctx, amount, side)?;

        emit!(StakeAndVoteCompleted {
            pool: pool_key,
            proposal: ctx.accounts.proposal.key(),
            user: ctx.accounts.owner.key(),
            stake_amount: amount,
            vote_side: side,
        });
        Ok(())
    }
}

fn update_rewards(pool: &mut Account<StakePool>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    if pool.total_staked > 0 && now > pool.last_update_ts {
        let elapsed = (now - pool.last_update_ts) as u128;
        let increment = elapsed
            .checked_mul(pool.reward_rate as u128)
            .ok_or(StakingError::MathOverflow)?
            .checked_mul(PRECISION)
            .ok_or(StakingError::MathOverflow)?
            .checked_div(pool.total_staked as u128)
            .ok_or(StakingError::MathOverflow)?;
        pool.accumulated_reward_per_token = pool
            .accumulated_reward_per_token
            .checked_add(increment)
            .ok_or(StakingError::MathOverflow)?;
    }
    pool.last_update_ts = now;
    Ok(())
}

fn calc_pending(amount: u64, accumulated: u128, debt: u128) -> Result<u64> {
    let total = (amount as u128)
        .checked_mul(accumulated)
        .ok_or(StakingError::MathOverflow)?;
    Ok(total
        .checked_sub(debt)
        .ok_or(StakingError::MathOverflow)?
        .checked_div(PRECISION)
        .ok_or(StakingError::MathOverflow)? as u64)
}

fn calc_debt(amount: u64, accumulated: u128) -> Result<u128> {
    (amount as u128)
        .checked_mul(accumulated)
        .ok_or_else(|| error!(StakingError::MathOverflow))
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    pub stake_mint: InterfaceAccount<'info, Mint>,
    pub reward_mint: InterfaceAccount<'info, Mint>,
    #[account(init, payer = authority, space = 8 + StakePool::INIT_SPACE,
        seeds = [b"pool", authority.key().as_ref(), stake_mint.key().as_ref()], bump)]
    pub pool: Account<'info, StakePool>,
    #[account(init, payer = authority, token::mint = stake_mint, token::authority = pool)]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(init, payer = authority, token::mint = reward_mint, token::authority = pool)]
    pub reward_vault: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, seeds = [b"pool", pool.authority.as_ref(), pool.stake_mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, StakePool>,
    #[account(init_if_needed, payer = owner, space = 8 + StakeEntry::INIT_SPACE,
        seeds = [b"stake", pool.key().as_ref(), owner.key().as_ref()], bump)]
    pub stake_entry: Account<'info, StakeEntry>,
    #[account(constraint = stake_mint.key() == pool.stake_mint)]
    pub stake_mint: InterfaceAccount<'info, Mint>,
    #[account(constraint = reward_mint.key() == pool.reward_mint)]
    pub reward_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, constraint = stake_vault.mint == pool.stake_mint, constraint = stake_vault.owner == pool.key())]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = reward_vault.mint == pool.reward_mint, constraint = reward_vault.owner == pool.key())]
    pub reward_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = user_stake_ata.mint == pool.stake_mint)]
    pub user_stake_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = user_reward_ata.mint == pool.reward_mint)]
    pub user_reward_ata: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, seeds = [b"pool", pool.authority.as_ref(), pool.stake_mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, StakePool>,
    #[account(mut, seeds = [b"stake", pool.key().as_ref(), owner.key().as_ref()], bump = stake_entry.bump,
        has_one = owner, has_one = pool)]
    pub stake_entry: Account<'info, StakeEntry>,
    #[account(constraint = stake_mint.key() == pool.stake_mint)]
    pub stake_mint: InterfaceAccount<'info, Mint>,
    #[account(constraint = reward_mint.key() == pool.reward_mint)]
    pub reward_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, constraint = stake_vault.mint == pool.stake_mint, constraint = stake_vault.owner == pool.key())]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = reward_vault.mint == pool.reward_mint, constraint = reward_vault.owner == pool.key())]
    pub reward_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = user_stake_ata.mint == pool.stake_mint)]
    pub user_stake_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = user_reward_ata.mint == pool.reward_mint)]
    pub user_reward_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, seeds = [b"pool", pool.authority.as_ref(), pool.stake_mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, StakePool>,
    #[account(mut, seeds = [b"stake", pool.key().as_ref(), owner.key().as_ref()], bump = stake_entry.bump,
        has_one = owner, has_one = pool)]
    pub stake_entry: Account<'info, StakeEntry>,
    #[account(constraint = reward_mint.key() == pool.reward_mint)]
    pub reward_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, constraint = reward_vault.mint == pool.reward_mint, constraint = reward_vault.owner == pool.key())]
    pub reward_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, constraint = user_reward_ata.mint == pool.reward_mint)]
    pub user_reward_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct UpdateRewardRate<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [b"pool", pool.authority.as_ref(), pool.stake_mint.as_ref()], bump = pool.bump, has_one = authority)]
    pub pool: Account<'info, StakePool>,
}

#[derive(Accounts)]
pub struct StakeAndVote<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    // ---- Staking accounts ----
    #[account(mut, seeds = [b"pool", pool.authority.as_ref(), pool.stake_mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, StakePool>,
    #[account(init_if_needed, payer = owner, space = 8 + StakeEntry::INIT_SPACE,
        seeds = [b"stake", pool.key().as_ref(), owner.key().as_ref()], bump)]
    pub stake_entry: Account<'info, StakeEntry>,
    #[account(constraint = stake_mint.key() == pool.stake_mint)]
    pub stake_mint: InterfaceAccount<'info, Mint>,
    #[account(constraint = reward_mint.key() == pool.reward_mint)]
    pub reward_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, constraint = stake_vault.mint == pool.stake_mint, constraint = stake_vault.owner == pool.key())]
    pub stake_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, constraint = reward_vault.mint == pool.reward_mint, constraint = reward_vault.owner == pool.key())]
    pub reward_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, constraint = user_stake_ata.mint == pool.stake_mint)]
    pub user_stake_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, constraint = user_reward_ata.mint == pool.reward_mint)]
    pub user_reward_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    // ---- DAO voting CPI accounts ----
    /// The DAO account in the dao-voting program.
    #[account(mut)]
    pub dao: Box<Account<'info, Dao>>,
    /// The proposal to vote on.
    #[account(mut)]
    pub proposal: Box<Account<'info, Proposal>>,
    /// The vote record PDA (will be init'd by the CPI).
    /// CHECK: Initialized by the dao-voting program via CPI.
    #[account(mut)]
    pub vote_record: UncheckedAccount<'info>,
    /// The governance token mint used by the DAO.
    #[account(constraint = governance_mint.key() == dao.governance_mint)]
    pub governance_mint: InterfaceAccount<'info, Mint>,
    /// The voter's governance token account (tokens to lock for the vote).
    #[account(mut, constraint = voter_governance_ata.mint == dao.governance_mint)]
    pub voter_governance_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The DAO's vote vault that holds locked governance tokens.
    #[account(mut, constraint = dao_vote_vault.owner == dao.key())]
    pub dao_vote_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The dao-voting program.
    pub dao_voting_program: Program<'info, SolanaDaoVoting>,

    // ---- Shared ----
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[account]
#[derive(InitSpace)]
pub struct StakePool {
    pub authority: Pubkey,
    pub stake_mint: Pubkey,
    pub reward_mint: Pubkey,
    pub total_staked: u64,
    pub reward_rate: u64,
    pub last_update_ts: i64,
    pub accumulated_reward_per_token: u128,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct StakeEntry {
    pub owner: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub reward_debt: u128,
    pub bump: u8,
}

#[error_code]
pub enum StakingError {
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Insufficient staked balance")]
    InsufficientStake,
    #[msg("No pending rewards")]
    NoPendingRewards,
}

#[event]
pub struct PoolInitialized {
    pub authority: Pubkey,
    pub staking_mint: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_rate: u64,
}

#[event]
pub struct TokensStaked {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
}

#[event]
pub struct TokensUnstaked {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
}

#[event]
pub struct RewardsClaimed {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
}

#[event]
pub struct StakeAndVoteCompleted {
    pub pool: Pubkey,
    pub proposal: Pubkey,
    pub user: Pubkey,
    pub stake_amount: u64,
    pub vote_side: bool,
}
