use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::ChubiError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketState>,

    /// CHECK: Vault PDA receives SOL.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        init,
        payer = maker,
        space = 8 + PositionState::INIT_SPACE,
        seeds = [
            b"position",
            market.key().as_ref(),
            maker.key().as_ref(),
            market.position_count.to_le_bytes().as_ref(),
        ],
        bump,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<Deposit>, side: u8, amount: u64, min_weight: u64) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    // ── Validation ──
    require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);
    require!((side as usize) < market.num_sides as usize, ChubiError::InvalidSide);
    require!(amount >= MIN_DEPOSIT_LAMPORTS, ChubiError::DepositTooSmall);

    let end_time = market.created_at + market.resolution_duration;
    require!(now < end_time, ChubiError::MarketExpired);

    // ── Compute fraction remaining (PRECISION-scaled) ──
    let time_remaining = (end_time - now) as u128;
    let total_duration = market.resolution_duration as u128;
    let fraction_remaining = (time_remaining * PRECISION as u128 / total_duration) as u64;

    // ── Dynamic lockout check ──
    if market.enable_lockout {
        let total_pos = math::total_positions(&market.side_position_counts, market.num_sides);
        let lockout_frac = math::compute_lockout(&market.pools, market.num_sides, total_pos);
        require!(fraction_remaining >= lockout_frac, ChubiError::InLockoutPeriod);
    }

    // ── Update TWD before pool changes ──
    math::update_twd(market, now)?;

    // ── Compute entry weight ──
    let entry_weight = math::compute_entry_weight(fraction_remaining, market.resolution_duration)?;

    // ── Slippage guard ──
    // `min_weight = 0` means "no guard". Callers should pass the weight they
    // showed to the user minus a small tolerance (e.g. 2%). If the lockout clock
    // or another deposit landed first and dropped the weight below that, revert
    // instead of silently giving a worse position.
    require!(entry_weight >= min_weight, ChubiError::WeightSlippage);

    // ── Transfer SOL: maker → vault ──
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.maker.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        amount,
    )?;

    // ── Initialize position ──
    let position = &mut ctx.accounts.position;
    position.bump = ctx.bumps.position;
    position.market = market.key();
    position.maker = ctx.accounts.maker.key();
    position.side = side;
    position.amount = amount;
    position.payout_amount = 0;
    position.nonce = market.position_count;
    position.claimed = false;
    position.created_at = now;
    position.entry_weight = entry_weight;
    position.withdrawn = false;

    // ── Update market state ──
    let s = side as usize;
    market.pools[s] = market.pools[s]
        .checked_add(amount).ok_or(error!(ChubiError::MathOverflow))?;
    market.weighted_pools[s] = market.weighted_pools[s]
        .checked_add((amount as u128).checked_mul(entry_weight as u128)
            .ok_or(error!(ChubiError::MathOverflow))?)
        .ok_or(error!(ChubiError::MathOverflow))?;
    market.side_position_counts[s] = market.side_position_counts[s]
        .checked_add(1).ok_or(error!(ChubiError::MathOverflow))?;
    market.total_deposited = market.total_deposited
        .checked_add(amount).ok_or(error!(ChubiError::MathOverflow))?;
    market.position_count = market.position_count
        .checked_add(1).ok_or(error!(ChubiError::MathOverflow))?;

    // Track first deposit timestamp
    if market.first_deposit_at[s] == 0 {
        market.first_deposit_at[s] = now;
    }

    // Recompute has_both_sides
    let mut sides_with = 0u8;
    for i in 0..market.num_sides as usize {
        if market.pools[i] > 0 {
            sides_with += 1;
        }
    }
    market.has_both_sides = sides_with >= 2;

    emit!(events::Deposited {
        market_id: market.market_id.clone(),
        maker: position.maker,
        side,
        amount,
        entry_weight,
        nonce: position.nonce,
        timestamp: now,
    });

    Ok(())
}
