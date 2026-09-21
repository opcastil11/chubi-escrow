use anchor_lang::prelude::*;
use crate::errors::BetError;
use crate::events;
use crate::math;
use crate::state::*;

/// Authority-only resolution. A bet market settles against a real-world
/// outcome, so there is nothing the chain can derive by itself — unlike
/// chubi-escrow's permissionless `resolve_market`, this one needs an oracle,
/// and the authority is it.
///
/// Callable at any time (mirroring chubi-escrow's `admin_resolve`), because
/// events routinely settle before the betting window was scheduled to close.
/// Resolving early simply stops further betting.
#[derive(Accounts)]
pub struct ResolveBetMarket<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, BetMarket>,

    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<ResolveBetMarket>, winner: u8) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(market.status == BetStatus::Open, BetError::MarketNotOpen);
    // winner is 1-indexed (1-6)
    require!(
        winner >= 1 && (winner as usize) <= market.num_sides as usize,
        BetError::InvalidWinner
    );

    // Need at least two backed outcomes: with only one, there is nothing to
    // win and settlement would just hand everyone their own money back minus
    // rent. `invalidate_market` + `refund` is the correct path for that.
    let mut sides_with = 0u8;
    for i in 0..market.num_sides as usize {
        if market.pools[i] > 0 {
            sides_with += 1;
        }
    }
    require!(sides_with >= 2, BetError::InsufficientSides);

    let winner_idx = (winner - 1) as usize;
    // Nobody backed the outcome that actually happened. The pot has no valid
    // claimant, so refuse to strand it — the authority must invalidate and
    // let everyone refund.
    require!(market.pools[winner_idx] > 0, BetError::NoWinningBets);

    market.status = BetStatus::Resolved;
    market.winner = winner;
    market.resolved_at = now;

    emit!(events::BetMarketResolved {
        market_id: market.market_id.clone(),
        winner,
        winner_pool: market.pools[winner_idx],
        total_pool: math::sum_pools(&market.pools, market.num_sides),
        resolver: ctx.accounts.authority.key(),
    });

    Ok(())
}
