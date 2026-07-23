use anchor_lang::prelude::*;
use crate::errors::BetError;
use crate::events;
use crate::state::*;

/// Void the market: the event didn't happen, was ambiguous, or nobody backed
/// the outcome that did. Every bet becomes refundable at full stake.
#[derive(Accounts)]
pub struct InvalidateBetMarket<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, BetMarket>,

    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<InvalidateBetMarket>) -> Result<()> {
    let market = &mut ctx.accounts.market;

    require!(market.status == BetStatus::Open, BetError::MarketNotOpen);

    market.status = BetStatus::Invalid;
    market.winner = 0;

    emit!(events::BetMarketInvalidated {
        market_id: market.market_id.clone(),
    });

    Ok(())
}
