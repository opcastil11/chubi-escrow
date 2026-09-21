use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
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

    /// Vault token account returns the stake. Owned by the market PDA.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
        token::mint = mint,
        token::authority = market,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    /// The SPL mint the market settles in.
    pub mint: InterfaceAccount<'info, Mint>,

    /// Bettor's token account, the destination of the refund.
    #[account(
        mut,
        token::mint = mint,
        token::authority = bettor,
    )]
    pub bettor_token: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = bet.market == market.key() @ BetError::BetMarketMismatch,
        constraint = bet.bettor == bettor.key() @ BetError::NotYourBet,
    )]
    pub bet: Account<'info, Bet>,

    #[account(mut)]
    pub bettor: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RefundBet>) -> Result<()> {
    require!(ctx.accounts.market.status == BetStatus::Invalid, BetError::MarketNotInvalid);
    require!(!ctx.accounts.bet.claimed, BetError::AlreadyClaimed);

    let refund_amount = ctx.accounts.bet.amount;

    // Vault token authority is the MARKET PDA → sign with the MARKET seeds.
    let market_id_bytes = ctx.accounts.market.market_id.as_bytes().to_vec();
    let market_bump = ctx.accounts.market.bump;
    let market_seeds: &[&[u8]] = &[
        b"market",
        market_id_bytes.as_ref(),
        std::slice::from_ref(&market_bump),
    ];

    token_interface::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.bettor_token.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            &[market_seeds],
        ),
        refund_amount,
        ctx.accounts.mint.decimals,
    )?;

    let bet = &mut ctx.accounts.bet;
    bet.claimed = true;
    bet.payout_amount = refund_amount;

    let market = &mut ctx.accounts.market;
    market.total_paid = market.total_paid
        .checked_add(refund_amount).ok_or(error!(BetError::MathOverflow))?;

    let market = &ctx.accounts.market;
    emit!(events::BetRefunded {
        market_id: market.market_id.clone(),
        bettor: ctx.accounts.bet.bettor,
        nonce: ctx.accounts.bet.nonce,
        amount: refund_amount,
    });

    Ok(())
}
