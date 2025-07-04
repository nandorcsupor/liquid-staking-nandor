use anchor_lang::prelude::*;
use anchor_lang::system_program::{System};
use anchor_spl::token::Token;
use anchor_spl::token::{Mint, TokenAccount};
use solana_program_option::COption;

declare_id!("4fLrcA8T6sH1z691Rv4JubkzqoNq9fjooaw4iKfjXzj3");

const STAKE_ACCOUNT_SIZE: usize = 200;

#[program]
pub mod liquid_staking {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        pool.authority = ctx.accounts.authority.key();
        pool.total_sol_deposited = 0;
        pool.total_fluidSOL_minted = 0;
        pool.exchange_rate = 1_000_000_000; // 1:1 initially
        pool.staked_sol_balance = 0;        // SOL staked to validators
        pool.liquid_reserve = 0;            // SOL kept for instant withdrawals
        pool.protocol_fees_earned = 0;      // Protocol revenue
        pool.bump = ctx.bumps.pool;
        pool.validator_count = 0;
        pool.target_reserve_ratio = 30;     // 30% reserve target
        pool.protocol_fee_bps = 1000;       // 10% fee in basis points
        
        msg!("FluidSOL liquid staking pool initialized!");
        msg!("Pool authority: {}", pool.authority);
        msg!("Target reserve ratio: {}%", pool.target_reserve_ratio);
        
        Ok(())
    }

    /// Add a validator to the pool's delegation strategy
    pub fn add_validator(
        ctx: Context<AddValidator>,
        validator_vote_account: Pubkey,
        allocation_percentage: u8,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        // Only authority can add validators
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        require!(allocation_percentage <= 100, ErrorCode::InvalidAllocation);
        require!(pool.validator_count < 10, ErrorCode::TooManyValidators); // Max 10 validators
        
        let validator_info = &mut ctx.accounts.validator_info;
        validator_info.vote_account = validator_vote_account;
        validator_info.allocation_percentage = allocation_percentage;
        validator_info.total_delegated = 0;
        validator_info.last_update_epoch = Clock::get()?.epoch;
        validator_info.performance_score = 100; // Start with perfect score
        validator_info.is_active = true;
        
        pool.validator_count += 1;
        
        msg!("Added validator: {}", validator_vote_account);
        msg!("Allocation: {}%", allocation_percentage);
        
        Ok(())
    }

    /// Deposit SOL and receive FluidSOL tokens
    pub fn deposit_sol(
        ctx: Context<DepositSol>,
        sol_amount: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        require!(sol_amount > 0, ErrorCode::InvalidAmount);
        require!(sol_amount >= 1_000_000, ErrorCode::MinimumDeposit); // 0.001 SOL minimum
        
        // Calculate FluidSOL tokens to mint
        let fluidSOL_to_mint = sol_amount
            .checked_mul(1_000_000_000)
            .unwrap()
            .checked_div(pool.exchange_rate)
            .unwrap();
        
        msg!("Depositing {} SOL for {} fSOL", 
             sol_amount as f64 / 1_000_000_000.0,
             fluidSOL_to_mint as f64 / 1_000_000_000.0);

        // Transfer SOL from user to pool
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.user.to_account_info(),
                to: pool.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, sol_amount)?;

        // Mint FluidSOL tokens to user
        let seeds = &[b"pool".as_ref(), &[pool.bump]];
        let signer = &[&seeds[..]];

        let cpi_accounts = anchor_spl::token::MintTo {
        mint: ctx.accounts.fluidSOL_mint.to_account_info(),
        to: ctx.accounts.user_fluidSOL_account.to_account_info(),
        authority: pool.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
        signer,
    );
        anchor_spl::token::mint_to(cpi_ctx, fluidSOL_to_mint)?;

        // Update pool state
        pool.total_sol_deposited = pool.total_sol_deposited.checked_add(sol_amount).unwrap();
        pool.total_fluidSOL_minted = pool.total_fluidSOL_minted.checked_add(fluidSOL_to_mint).unwrap();
        
        // Add to liquid reserve initially (will be rebalanced later)
        pool.liquid_reserve = pool.liquid_reserve.checked_add(sol_amount).unwrap();

        msg!("Deposit successful! Pool balance: {} SOL", 
             pool.total_sol_deposited as f64 / 1_000_000_000.0);

        Ok(())
    }

    /// Withdraw SOL by burning FluidSOL tokens (instant if reserve available)
    pub fn withdraw_sol(
        ctx: Context<WithdrawSol>,
        fluidSOL_amount: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        // Validate withdrawal amount
        require!(fluidSOL_amount > 0, ErrorCode::InvalidAmount);
        
        // Calculate SOL to return based on current exchange rate
        let sol_to_return = fluidSOL_amount
            .checked_mul(pool.exchange_rate)
            .unwrap()
            .checked_div(1_000_000_000)
            .unwrap();
        
        // Check if we have enough in liquid reserve for instant withdrawal
        require!(sol_to_return <= pool.liquid_reserve, ErrorCode::InsufficientLiquidity);
        
        // Calculate 0.3% instant withdrawal fee
        let withdrawal_fee = sol_to_return.checked_mul(30).unwrap().checked_div(10000).unwrap();
        let net_sol_to_user = sol_to_return.checked_sub(withdrawal_fee).unwrap();
        
        msg!("Withdrawing {} fSOL for {} SOL (fee: {} SOL)", 
            fluidSOL_amount as f64 / 1_000_000_000.0,
            net_sol_to_user as f64 / 1_000_000_000.0,
            withdrawal_fee as f64 / 1_000_000_000.0);

        // Burn FluidSOL tokens from user's account
        let cpi_accounts = anchor_spl::token::Burn {
            mint: ctx.accounts.fluidSOL_mint.to_account_info(),
            from: ctx.accounts.user_fluidSOL_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        anchor_spl::token::burn(cpi_ctx, fluidSOL_amount)?;

        // Transfer SOL from pool to user (direct lamport manipulation - pool has data)
        **pool.to_account_info().try_borrow_mut_lamports()? -= net_sol_to_user;
        **ctx.accounts.user.to_account_info().try_borrow_mut_lamports()? += net_sol_to_user;

        // Update pool accounting
        pool.total_sol_deposited = pool.total_sol_deposited.checked_sub(sol_to_return).unwrap();
        pool.total_fluidSOL_minted = pool.total_fluidSOL_minted.checked_sub(fluidSOL_amount).unwrap();
        pool.liquid_reserve = pool.liquid_reserve.checked_sub(sol_to_return).unwrap();
        pool.protocol_fees_earned = pool.protocol_fees_earned.checked_add(withdrawal_fee).unwrap();

        msg!("Withdrawal successful! Remaining pool reserve: {} SOL", 
            pool.liquid_reserve as f64 / 1_000_000_000.0);

        Ok(())
    }

    pub fn stake_to_validator(
        ctx: Context<StakeToValidator>,
        amount: u64,
        slot: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;

        msg!("🔍 STEP 0: Starting stake_to_validator");
        msg!("🔍 Pool liquid reserve: {}", pool.liquid_reserve);
        msg!("🔍 Stake amount requested: {}", amount);

        // Authority and validation checks
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        require!(amount <= pool.liquid_reserve, ErrorCode::InsufficientLiquidity);
        // require!(validator_index < pool.validator_count, ErrorCode::InvalidValidatorIndex);
        require!(slot > 0, ErrorCode::InvalidValidatorIndex);
        
        let validator_info = &mut ctx.accounts.validator_info;
        require!(validator_info.is_active, ErrorCode::ValidatorInactive);

        // Calculate rent-exempt minimum (stake account already has rent from init)
        let rent = Rent::get()?;
        let stake_account_rent = rent.minimum_balance(STAKE_ACCOUNT_SIZE);

        msg!("🔍 STEP 1: Stake account created by Anchor");
        msg!("🔍 Stake space: {}", STAKE_ACCOUNT_SIZE);
        msg!("🔍 Rent in account: {}", stake_account_rent);
        msg!("🔍 Stake account (PDA): {}", ctx.accounts.stake_account.key());

        // Pool authority seeds for signing
        let pool_seeds = &[b"pool".as_ref(), &[pool.bump]];
        
        let pool_signer = &[&pool_seeds[..]];

        // STEP 1: Initialize stake account (Anchor already created it as system account)
        msg!("🔍 STEP 1: Initializing stake account...");
        let authorized = anchor_lang::solana_program::stake::state::Authorized {
            staker: pool.key(),
            withdrawer: pool.key(),
        };
        let initialize_ix = anchor_lang::solana_program::stake::instruction::initialize(
            &ctx.accounts.stake_account.key(),
            &authorized,
            &anchor_lang::solana_program::stake::state::Lockup::default(),
        );
        anchor_lang::solana_program::program::invoke(
            &initialize_ix,
            &[
                ctx.accounts.stake_account.to_account_info(),
                ctx.accounts.rent.to_account_info(),
            ],
        )?;
        msg!("✅ STEP 1 SUCCESS: Stake account initialized!");

        // STEP 2: Transfer staking amount from pool to stake account
        msg!("🔍 STEP 2: Transferring {} lamports from pool to stake account...", amount);

        // Direct lamport transfer - pool has data so can't use system program
        **pool.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.stake_account.to_account_info().try_borrow_mut_lamports()? += amount;

        msg!("✅ STEP 2 SUCCESS: Amount transferred to stake account!");

        msg!("💎 BALANCES AFTER TRANSFER:");
        msg!("  pool balance: {}", pool.to_account_info().lamports());
        msg!("  stake_account balance: {}", ctx.accounts.stake_account.to_account_info().lamports());
        msg!("  stake_account_rent_requirement: {}", stake_account_rent);

        // STEP 3: Delegate stake to validator
        msg!("🔍 STEP 3: Delegating stake to validator...");
        // TODO - Fix: Incorrect Program ID for instruction here somewhere.
        // msg! logs these accounts below.!
        msg!("💎 Before DELEGATE IX 1 {}", &ctx.accounts.stake_account.key());
        msg!("💎 Before DELEGATE IX 2 {}", &pool.key());
        msg!("💎 Before DELEGATE IX 3 {}", &ctx.accounts.validator_vote_account.key());
        let delegate_ix = anchor_lang::solana_program::stake::instruction::delegate_stake(
            &ctx.accounts.stake_account.key(),
            &pool.key(), // Pool is the staker authority
            &ctx.accounts.validator_vote_account.key(),
        );

        // Log instruction details
        msg!("💎 DELEGATE IX CREATED:");
        msg!("  program_id: {}", delegate_ix.program_id);
        msg!("  accounts_len: {}", delegate_ix.accounts.len());

        // Log accounts being passed to invoke_signed
        msg!("💎 INVOKE_SIGNED ACCOUNTS:");
        msg!("  [0] stake_account: {} (owner: {})", 
            ctx.accounts.stake_account.key(), 
            ctx.accounts.stake_account.owner);
        msg!("  [1] vote_account: {}", ctx.accounts.validator_vote_account.key());
        msg!("  [2] clock: {}", ctx.accounts.clock.key());
        msg!("  [3] stake_history: {}", ctx.accounts.stake_history.key());
        msg!("  [4] stake_config: {}", ctx.accounts.stake_config.key());
        msg!("  [5] pool (authority): {}", pool.key());

        // Log signer seeds
        msg!("💎 SIGNER SEEDS:");
        msg!("  pool_bump: {}", pool.bump);

        msg!("🔍 TESTING PDA DERIVATION:");
        let (derived_pool, derived_bump) = Pubkey::find_program_address(
            &[b"pool"], 
            &crate::ID
        );
        msg!("  derived_pool: {}", derived_pool);
        msg!("  actual_pool: {}", pool.key());
        msg!("  derived_bump: {}", derived_bump);
        msg!("  stored_bump: {}", pool.bump);


        // CPI - Cross-Program Invocation - signs "on behalf of someone, like PDA"
        anchor_lang::solana_program::program::invoke_signed(
            &delegate_ix,
            &[
                ctx.accounts.stake_account.to_account_info(),
                ctx.accounts.validator_vote_account.to_account_info(),
                ctx.accounts.clock.to_account_info(),
                ctx.accounts.stake_history.to_account_info(),
                ctx.accounts.stake_config.to_account_info(),
                pool.to_account_info(), // Pool signs as staker
            ],
            pool_signer,
        )?;
        msg!("✅ STEP 3 SUCCESS: Stake delegated to validator!");

        // STEP 4: Update accounting
        msg!("🔍 STEP 4: Updating pool accounting...");
        // Only subtract the staking amount, not rent (authority already paid rent)
        pool.liquid_reserve = pool.liquid_reserve.checked_sub(amount).unwrap();
        pool.staked_sol_balance = pool.staked_sol_balance.checked_add(amount).unwrap();
        validator_info.total_delegated = validator_info.total_delegated.checked_add(amount).unwrap();
        validator_info.last_update_epoch = Clock::get()?.epoch;

        msg!("✅ VALÓDI STAKING SUCCESSFUL! {} SOL delegated!", amount as f64 / 1_000_000_000.0);
        
        Ok(())
    }

    /// 🔥 NEW: Harvest rewards from specific validator
    pub fn harvest_rewards(
        ctx: Context<HarvestRewards>,
        validator_index: u8,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        require!(validator_index < pool.validator_count, ErrorCode::InvalidValidatorIndex);
        
        let validator_info = &mut ctx.accounts.validator_info;
        let stake_account_balance = ctx.accounts.stake_account.to_account_info().lamports();
        
        msg!("🌾 Checking rewards for validator {}", validator_index);
        
        // Calculate rewards (current balance - original delegation)
        if stake_account_balance > validator_info.total_delegated {
            let rewards_earned = stake_account_balance.checked_sub(validator_info.total_delegated).unwrap();
            
            msg!("🎉 Found {} SOL rewards from validator!", rewards_earned as f64 / 1_000_000_000.0);
            
            // Calculate protocol fee (10%)
            let protocol_fee = rewards_earned
                .checked_mul(pool.protocol_fee_bps as u64)
                .unwrap()
                .checked_div(10000)
                .unwrap();
            
            let user_rewards = rewards_earned.checked_sub(protocol_fee).unwrap();
            
            // Update pool accounting
            pool.staked_sol_balance = pool.staked_sol_balance.checked_add(user_rewards).unwrap();
            pool.protocol_fees_earned = pool.protocol_fees_earned.checked_add(protocol_fee).unwrap();
            pool.total_sol_deposited = pool.total_sol_deposited.checked_add(user_rewards).unwrap();
            
            // Update exchange rate - FluidSOL now worth more!
            if pool.total_fluidSOL_minted > 0 {
                pool.exchange_rate = pool.total_sol_deposited
                    .checked_mul(1_000_000_000)
                    .unwrap()
                    .checked_div(pool.total_fluidSOL_minted)
                    .unwrap();
            }
            
            // Update validator tracking
            validator_info.total_delegated = stake_account_balance;
            validator_info.last_update_epoch = Clock::get()?.epoch;
            
            msg!("💎 New exchange rate: {}", pool.exchange_rate as f64 / 1_000_000_000.0);
            msg!("🎯 Protocol earned {} SOL", protocol_fee as f64 / 1_000_000_000.0);
            
        } else {
            msg!("⏳ No new rewards from this validator yet");
        }
        
        Ok(())
    }

    /// Update rewards from validators and adjust exchange rate
    pub fn update_rewards(
        ctx: Context<UpdateRewards>,
        total_rewards_earned: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        require!(total_rewards_earned > 0, ErrorCode::InvalidAmount);
        
        // Calculate protocol fee (10% of rewards)
        let protocol_fee = total_rewards_earned
            .checked_mul(pool.protocol_fee_bps as u64)
            .unwrap()
            .checked_div(10000)
            .unwrap();
        
        let user_rewards = total_rewards_earned.checked_sub(protocol_fee).unwrap();
        
        // Add rewards to pool (90% to users via exchange rate, 10% to protocol)
        pool.staked_sol_balance = pool.staked_sol_balance.checked_add(user_rewards).unwrap();
        pool.protocol_fees_earned = pool.protocol_fees_earned.checked_add(protocol_fee).unwrap();
        pool.total_sol_deposited = pool.total_sol_deposited.checked_add(user_rewards).unwrap();
        
        // Update exchange rate - more SOL backing same FluidSOL tokens
        if pool.total_fluidSOL_minted > 0 {
            pool.exchange_rate = pool.total_sol_deposited
                .checked_mul(1_000_000_000)
                .unwrap()
                .checked_div(pool.total_fluidSOL_minted)
                .unwrap();
        }
        
        msg!("Rewards updated: {} SOL total, {} SOL to users, {} SOL protocol fee", 
             total_rewards_earned as f64 / 1_000_000_000.0,
             user_rewards as f64 / 1_000_000_000.0,
             protocol_fee as f64 / 1_000_000_000.0);
        msg!("New exchange rate: {}", pool.exchange_rate as f64 / 1_000_000_000.0);
        
        Ok(())
    }

    /// Rebalance pool to maintain target reserve ratio
    pub fn rebalance_pool(
        ctx: Context<RebalancePool>,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        
        let total_balance = pool.liquid_reserve.checked_add(pool.staked_sol_balance).unwrap();
        let current_reserve_ratio = if total_balance > 0 {
            pool.liquid_reserve.checked_mul(100).unwrap().checked_div(total_balance).unwrap()
        } else {
            0
        };
        
        let target_reserve = total_balance
            .checked_mul(pool.target_reserve_ratio as u64)
            .unwrap()
            .checked_div(100)
            .unwrap();
        
        msg!("Current reserve ratio: {}%, target: {}%", 
             current_reserve_ratio, pool.target_reserve_ratio);
        
        if pool.liquid_reserve < target_reserve {
            // Need to unstake from validators
            let amount_to_unstake = target_reserve.checked_sub(pool.liquid_reserve).unwrap();
            msg!("Need to unstake {} SOL from validators", 
                 amount_to_unstake as f64 / 1_000_000_000.0);
            
            // In full implementation, this would initiate unstaking
            // For now, we'll simulate immediate unstaking (devnet testing)
            if amount_to_unstake <= pool.staked_sol_balance {
                pool.staked_sol_balance = pool.staked_sol_balance.checked_sub(amount_to_unstake).unwrap();
                pool.liquid_reserve = pool.liquid_reserve.checked_add(amount_to_unstake).unwrap();
            }
        } else if pool.liquid_reserve > target_reserve {
            // Need to stake more to validators  
            let amount_to_stake = pool.liquid_reserve.checked_sub(target_reserve).unwrap();
            msg!("Should stake {} SOL to validators", 
                 amount_to_stake as f64 / 1_000_000_000.0);
            
            // This would be handled by stake_to_validators function
        }
        
        Ok(())
    }

    /// Withdraw protocol fees (authority only)
    pub fn withdraw_protocol_fees(
        ctx: Context<WithdrawProtocolFees>,
        amount: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        require!(ctx.accounts.authority.key() == pool.authority, ErrorCode::Unauthorized);
        require!(amount <= pool.protocol_fees_earned, ErrorCode::InsufficientFunds);
        
        // Transfer fees to authority
        let pool_seeds = &[b"pool".as_ref(), &[pool.bump]];
        let pool_signer = &[&pool_seeds[..]];

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: pool.to_account_info(),
                to: ctx.accounts.authority.to_account_info(),
            },
            pool_signer, // Pool PDA signs the transfer
        );
        anchor_lang::system_program::transfer(cpi_context, amount)?;
        
        pool.protocol_fees_earned = pool.protocol_fees_earned.checked_sub(amount).unwrap();
        
        msg!("Withdrew {} SOL protocol fees", amount as f64 / 1_000_000_000.0);
        
        Ok(())
    }
}

// ============================================================================
// ACCOUNT STRUCTURES
// ============================================================================

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        init,
        payer = authority,
        space = 200,
        seeds = [b"pool"],
        bump
    )]
    pub pool: Account<'info, StakingPool>,
    
    #[account(
        init,
        payer = authority,
        mint::decimals = 9,
        mint::authority = pool,
    )]
    pub fluidSOL_mint: Account<'info, Mint>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AddValidator<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(mut)]
    pub pool: Account<'info, StakingPool>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 1 + 8 + 8 + 1 + 1, // ValidatorInfo structure
        seeds = [b"validator", pool.key().as_ref(), &[pool.validator_count]],
        bump
    )]
    pub validator_info: Account<'info, ValidatorInfo>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
    
    #[account(
    mut,
    constraint = fluidSOL_mint.mint_authority == COption::Some(pool.key()) @ ErrorCode::InvalidMint
    )]
    pub fluidSOL_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user_fluidSOL_account: Account<'info, TokenAccount>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawSol<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
    
    #[account(
    mut,
    constraint = fluidSOL_mint.mint_authority == COption::Some(pool.key()) @ ErrorCode::InvalidMint
    )]
    pub fluidSOL_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user_fluidSOL_account: Account<'info, TokenAccount>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct StakeToValidators<'info> {
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
}

#[derive(Accounts)]
pub struct UpdateRewards<'info> {
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
}

#[derive(Accounts)]
pub struct RebalancePool<'info> {
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
}

#[derive(Accounts)]
pub struct WithdrawProtocolFees<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64, slot: u64)]
pub struct StakeToValidator<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
    
    #[account(mut)]
    pub validator_info: Account<'info, ValidatorInfo>,

    /// CHECK: The stake account is initialized by the program
    #[account(
        init,
        payer = authority,
        seeds = [b"stake", authority.key().as_ref(), &slot.to_le_bytes()],
        bump,
        space = STAKE_ACCOUNT_SIZE,
        owner = anchor_lang::solana_program::stake::program::ID
    )]
    pub stake_account: AccountInfo<'info>,
    
    /// CHECK: This is the validator's vote account
    pub validator_vote_account: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
    
    /// CHECK: Solana's native stake program
    #[account(address = anchor_lang::solana_program::stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    
    pub rent: Sysvar<'info, Rent>,
    pub clock: Sysvar<'info, Clock>,
    
    /// CHECK: Solana native stake history sysvar
    pub stake_history: AccountInfo<'info>,
    
    /// CHECK: Solana native stake config account
    pub stake_config: AccountInfo<'info>,
}

// 🔥 NEW: Harvest rewards from specific validator
#[derive(Accounts)]
pub struct HarvestRewards<'info> {
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, StakingPool>,
    
    #[account(mut)]
    pub validator_info: Account<'info, ValidatorInfo>,
    
    /// CHECK: The stake account to check for rewards
    pub stake_account: AccountInfo<'info>,
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[account]
pub struct StakingPool {
    pub authority: Pubkey,
    pub total_sol_deposited: u64,       // Total SOL in pool
    pub total_fluidSOL_minted: u64,     // Total FluidSOL tokens minted
    pub exchange_rate: u64,             // How much SOL per FluidSOL token
    pub staked_sol_balance: u64,        // SOL currently staked to validators (70%)
    pub liquid_reserve: u64,            // SOL kept for instant withdrawals (30%)
    pub protocol_fees_earned: u64,      // Protocol revenue (10% of validator rewards)
    pub bump: u8,
    pub validator_count: u8,            // Number of validators in strategy
    pub target_reserve_ratio: u8,       // Target % for liquid reserve (30)
    pub protocol_fee_bps: u16,          // Protocol fee in basis points (1000 = 10%)
}

#[account]
pub struct ValidatorInfo {
    pub vote_account: Pubkey,           // Validator's vote account
    pub allocation_percentage: u8,      // % of stake to allocate to this validator
    pub total_delegated: u64,           // Total SOL currently delegated
    pub last_update_epoch: u64,         // Last epoch we checked performance
    pub performance_score: u8,          // Performance score (0-100)
    pub is_active: bool,                // Whether validator is active
}

// ============================================================================
// ERROR CODES
// ============================================================================

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid amount provided")]
    InvalidAmount,
    
    #[msg("Minimum deposit is 0.001 SOL")]
    MinimumDeposit,
    
    #[msg("Insufficient funds in pool")]
    InsufficientFunds,
    
    #[msg("Insufficient liquidity for operation")]
    InsufficientLiquidity,
    
    #[msg("Unauthorized: only pool authority can perform this action")]
    Unauthorized,
    
    #[msg("Invalid exchange rate: must be >= 1.0")]
    InvalidExchangeRate,
    
    #[msg("Invalid allocation percentage")]
    InvalidAllocation,
    
    #[msg("Too many validators (max 10)")]
    TooManyValidators,

    #[msg("Invalid mint account")]
    InvalidMint,
    #[msg("Invalid token account")]
    InvalidTokenAccount,

    #[msg("Invalid validator index")]
    InvalidValidatorIndex,
    
    #[msg("Validator is not active")]
    ValidatorInactive,
}