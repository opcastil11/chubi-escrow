use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::BetError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct RefundBet<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: Vault PDA returns the stake.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        constraint = bet.market == market.key() @ BetError::BetMarketMismatch,
        constraint = bet.bettor == bettor.key() @ BetError::NotYourBet,
    )]
    pub bet: Account<'info, Bet>,

    #[account(mut)]
    pub bettor: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RefundBet>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let bet = &mut ctx.accounts.bet;

    require!(market.status == BetStatus::Invalid, BetError::MarketNotInvalid);
    require!(!bet.claimed, BetError::AlreadyClaimed);

    let refund_amount = bet.amount;

    let market_key = market.key();
    let vault_seeds = &[
        b"vault" as &[u8],
        market_key.as_ref(),
        &[market.vault_bump],
    ];

    system_program::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.bettor.to_account_info(),
            },
            &[vault_seeds],
        ),
        refund_amount,
    )?;

    bet.claimed = true;
    bet.payout_amount = refund_amount;
    market.total_paid = market.total_paid
        .checked_add(refund_amount).ok_or(error!(BetError::MathOverflow))?;

    emit!(events::BetRefunded {
        market_id: market.market_id.clone(),
        bettor: bet.bettor,
        nonce: bet.nonce,
        amount: refund_amount,
    });

    Ok(())
}
