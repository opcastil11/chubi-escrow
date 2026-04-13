use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::BPS_SCALE;
use crate::errors::ChubiError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketState>,

    /// CHECK: Vault PDA sends SOL.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
        constraint = position.maker == maker.key() @ ChubiError::NotYourPosition,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<Withdraw>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let position = &mut ctx.accounts.position;
    let now = Clock::get()?.unix_timestamp;

    require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);
    require!(market.allow_withdrawal, ChubiError::WithdrawalsNotAllowed);
    require!(!position.claimed, ChubiError::AlreadyClaimed);
    require!(!position.withdrawn, ChubiError::AlreadyWithdrawn);

    // Check lockout
    let elapsed = now.saturating_sub(market.created_at);
    if market.enable_lockout {
        let total_duration = market.resolution_duration;
        let time_remaining = (market.created_at + total_duration).saturating_sub(now);
        let fraction_remaining = if total_duration > 0 {
            (time_remaining as u128 * crate::constants::PRECISION as u128
                / total_duration as u128) as u64
        } else {
            0
        };
        let total_pos = math::total_positions(&market.side_position_counts, market.num_sides);
        let lockout_frac = math::compute_lockout(&market.pools, market.num_sides, total_pos);
        require!(fraction_remaining >= lockout_frac, ChubiError::InLockoutPeriod);
    }

    // Update TWD before pool changes
    math::update_twd(market, now)?;

    // Compute penalty
    let penalty_bps = math::compute_withdrawal_penalty(elapsed, market.resolution_duration);
    let penalty_amount = (position.amount as u128)
        .checked_mul(penalty_bps as u128).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;
    let return_amount = position.amount
        .checked_sub(penalty_amount).ok_or(error!(ChubiError::MathOverflow))?;

    // Transfer return_amount from vault → maker
    let market_key = market.key();
    let vault_seeds = &[
        b"vault" as &[u8],
        market_key.as_ref(),
        &[market.vault_bump],
    ];

    if return_amount > 0 {
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.maker.to_account_info(),
                },
                &[vault_seeds],
            ),
            return_amount,
        )?;
    }

    // Update market pools
    let s = position.side as usize;
    market.pools[s] = market.pools[s].saturating_sub(position.amount);
    let weighted_contrib = (position.amount as u128)
        .checked_mul(position.entry_weight as u128).ok_or(error!(ChubiError::MathOverflow))?;
    market.weighted_pools[s] = market.weighted_pools[s].saturating_sub(weighted_contrib);
    market.side_position_counts[s] = market.side_position_counts[s].saturating_sub(1);
    market.penalty_pool = market.penalty_pool
        .checked_add(penalty_amount).ok_or(error!(ChubiError::MathOverflow))?;

    // Recompute has_both_sides
    let mut sides_with = 0u8;
    for i in 0..market.num_sides as usize {
        if market.pools[i] > 0 {
            sides_with += 1;
        }
    }
    market.has_both_sides = sides_with >= 2;

    // Mark position
    position.withdrawn = true;

    emit!(events::Withdrawn {
        market_id: market.market_id.clone(),
        maker: position.maker,
        nonce: position.nonce,
        amount_returned: return_amount,
        penalty_amount,
        penalty_bps,
    });

    Ok(())
}
