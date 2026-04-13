use anchor_lang::prelude::*;
use crate::errors::ChubiError;
use crate::events;
use crate::math;
use crate::state::*;

/// Authority-only force resolution — can be called at any time.
/// Winner specified by authority (disputes, edge cases).
#[derive(Accounts)]
pub struct AdminResolve<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, MarketState>,

    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<AdminResolve>, winner: u8) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);
    // winner is 1-indexed (1-6)
    require!(
        winner >= 1 && (winner as usize) <= market.num_sides as usize,
        ChubiError::InvalidWinner
    );

    let winner_idx = winner - 1; // 0-indexed

    // Need at least 2 sides with positions
    let mut sides_with = 0u8;
    for i in 0..market.num_sides as usize {
        if market.side_position_counts[i] > 0 {
            sides_with += 1;
        }
    }
    require!(sides_with >= 2, ChubiError::InsufficientSides);

    // Finalize TWD
    math::update_twd(market, now)?;

    // Compute final twd_0
    let twd_0 = if market.cumulative_time > 0 {
        (market.cumulative_twd_0 / market.cumulative_time as u128) as u64
    } else {
        crate::constants::PRECISION / 2
    };

    // Compute TWCD (uses the authority-specified winner)
    let winner_payout_share = math::compute_twcd(
        winner_idx,
        twd_0,
        &market.pools,
        market.num_sides,
    )?;

    // Store resolution data
    market.status = MarketStatus::Resolved;
    market.winner = winner; // already 1-indexed
    market.twd_0 = twd_0;
    market.winner_payout_share = winner_payout_share;
    market.resolved_at = now;

    let total_pool = math::sum_pools(&market.pools, market.num_sides);

    emit!(events::MarketResolved {
        market_id: market.market_id.clone(),
        winner,
        twd_0,
        winner_payout_share,
        total_pool,
        penalty_pool: market.penalty_pool,
        resolver: ctx.accounts.authority.key(),
    });

    Ok(())
}
