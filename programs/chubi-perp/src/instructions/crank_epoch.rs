use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::PerpError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct CrankEpoch<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Anyone can crank — no signer restriction. Soft-incentive: the
    /// dominant side's holders gain proportionally to their weight share, so
    /// a holder cranking their own market is acting in self-interest.
    pub cranker: Signer<'info>,
}

pub fn handler(ctx: Context<CrankEpoch>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(!market.is_closed, PerpError::MarketClosed);
    require!(
        now - market.last_epoch_at >= EPOCH_SECS,
        PerpError::EpochNotElapsed
    );

    // compute_funding errors if pools are empty / balanced — those are valid
    // states that just don't produce funding. Bubble the error so the caller
    // can stop polling for a quiet market.
    let (winner_side, loser_side, funding, imbalance_bps) = math::compute_funding(&market.pools)?;

    market.pools[loser_side as usize] = market.pools[loser_side as usize]
        .checked_sub(funding).ok_or(error!(PerpError::MathOverflow))?;
    market.pools[winner_side as usize] = market.pools[winner_side as usize]
        .checked_add(funding).ok_or(error!(PerpError::MathOverflow))?;
    // weighted_pools intentionally NOT touched. Holders' weighted_share stays
    // constant; only the lamports denominator (pools[]) changes, so each
    // holder's exit value moves with funding flow proportional to their weight.

    market.cumulative_funding_paid = market.cumulative_funding_paid
        .checked_add(funding).ok_or(error!(PerpError::MathOverflow))?;
    market.last_epoch_at = now;

    emit!(events::EpochCranked {
        market_id: market.market_id.clone(),
        winner_side,
        funding_lamports: funding,
        imbalance_bps,
        cranker: ctx.accounts.cranker.key(),
        timestamp: now,
    });

    Ok(())
}
