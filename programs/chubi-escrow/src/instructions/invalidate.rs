use anchor_lang::prelude::*;
use crate::errors::ChubiError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct InvalidateMarket<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, MarketState>,

    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<InvalidateMarket>) -> Result<()> {
    let market = &mut ctx.accounts.market;

    require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);

    market.status = MarketStatus::Invalid;
    market.winner = 0;

    emit!(events::MarketInvalidated {
        market_id: market.market_id.clone(),
    });

    Ok(())
}
