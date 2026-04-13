use anchor_lang::prelude::*;
use crate::errors::ChubiError;
use crate::state::*;

#[derive(Accounts)]
pub struct ClosePosition<'info> {
    #[account(
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketState>,

    #[account(
        mut,
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
        constraint = position.maker == maker.key() @ ChubiError::NotYourPosition,
        close = maker,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,
}

pub fn handler(ctx: Context<ClosePosition>) -> Result<()> {
    let position = &ctx.accounts.position;

    // Must be settled (claimed, withdrawn, or refunded) before closing
    require!(
        position.claimed || position.withdrawn,
        ChubiError::PositionNotSettled
    );

    // Account is closed via the `close = maker` constraint above.
    // Rent is returned to maker automatically.

    Ok(())
}
