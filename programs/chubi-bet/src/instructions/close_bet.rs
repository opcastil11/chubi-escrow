use anchor_lang::prelude::*;
use crate::errors::BetError;
use crate::state::*;

#[derive(Accounts)]
pub struct CloseBet<'info> {
    #[account(
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    #[account(
        mut,
        constraint = bet.market == market.key() @ BetError::BetMarketMismatch,
        constraint = bet.bettor == bettor.key() @ BetError::NotYourBet,
        close = bettor,
    )]
    pub bet: Account<'info, Bet>,

    #[account(mut)]
    pub bettor: Signer<'info>,
}

pub fn handler(ctx: Context<CloseBet>) -> Result<()> {
    let market = &ctx.accounts.market;
    let bet = &ctx.accounts.bet;

    // A losing bet can never be claimed, so "claimed" alone would strand its
    // rent forever. Once the market is resolved against it, it is settled.
    let lost = market.status == BetStatus::Resolved
        && market.winner > 0
        && bet.side != market.winner - 1;

    require!(bet.claimed || lost, BetError::BetNotSettled);

    // Rent returns to the bettor via the `close = bettor` constraint.
    Ok(())
}
