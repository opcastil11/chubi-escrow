use anchor_lang::prelude::*;
use crate::errors::PerpError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct ClosePerpMarket<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
        constraint = market.authority == authority.key() @ PerpError::NotPerpetualAdmin,
    )]
    pub market: Account<'info, PerpMarketState>,

    pub authority: Signer<'info>,
}

/// Authority-only safety hatch. After close: no more deposits, no more
/// crank_epochs, but holders can still call `exit_perpetual` to recover their
/// fair-value share. The pool snapshot at close-time is what they exit against.
pub fn handler(ctx: Context<ClosePerpMarket>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    require!(!market.is_closed, PerpError::MarketClosed);
    market.is_closed = true;

    emit!(events::PerpClosed {
        market_id: market.market_id.clone(),
        authority: ctx.accounts.authority.key(),
    });

    Ok(())
}
