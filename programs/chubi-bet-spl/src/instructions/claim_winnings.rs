use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
use crate::constants::*;
use crate::errors::BetError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct ClaimWinnings<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// Vault token account pays the winnings. Owned by the market PDA.
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

    /// Winner's token account, the destination of the payout.
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

    #[account(
        mut,
        seeds = [b"creator", market.key().as_ref()],
        bump = creator_account.bump,
        constraint = creator_account.market == market.key() @ BetError::CreatorMarketMismatch,
    )]
    pub creator_account: Account<'info, BetCreator>,

    #[account(mut)]
    pub bettor: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimWinnings>) -> Result<()> {
    require!(ctx.accounts.market.status == BetStatus::Resolved, BetError::MarketNotResolved);
    require!(!ctx.accounts.bet.claimed, BetError::AlreadyClaimed);

    // winner is 1-indexed, bet.side is 0-indexed
    let winner_idx = ctx.accounts.market.winner
        .checked_sub(1).ok_or(error!(BetError::InvalidWinner))?;
    // Losers get nothing — that is the whole of parimutuel. They can still
    // recover their rent with `close_bet`.
    require!(ctx.accounts.bet.side == winner_idx, BetError::NoPayout);

    let total_pool = math::sum_pools(&ctx.accounts.market.pools, ctx.accounts.market.num_sides);
    let winner_pool = ctx.accounts.market.pools[winner_idx as usize];

    // Anonymous / house markets carry a side-car with creator = default and
    // pay no commission.
    let creator_fee_bps = if ctx.accounts.creator_account.creator != Pubkey::default() {
        CREATOR_FEE_BPS
    } else {
        0
    };

    let (payout, fee, creator_fee) =
        math::parimutuel_payout(ctx.accounts.bet.amount, winner_pool, total_pool, creator_fee_bps)?;
    require!(payout > 0, BetError::NoPayout);

    // The vault is a token account; its settleable balance is simply the token
    // amount it holds. No rent floor to reserve — token accounts keep their
    // lamport rent regardless of token balance.
    let available = ctx.accounts.vault.amount;
    let actual_payout = payout.min(available);
    require!(actual_payout > 0, BetError::InsufficientVault);

    // The vault's token authority is the MARKET PDA, so the signer seeds are
    // the MARKET seeds, not the vault seeds.
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
        actual_payout,
        ctx.accounts.mint.decimals,
    )?;

    let bet = &mut ctx.accounts.bet;
    bet.claimed = true;
    bet.payout_amount = actual_payout;

    let market = &mut ctx.accounts.market;
    market.total_paid = market.total_paid
        .checked_add(actual_payout).ok_or(error!(BetError::MathOverflow))?;
    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_add(fee).ok_or(error!(BetError::MathOverflow))?;

    if creator_fee > 0 {
        let creator_account = &mut ctx.accounts.creator_account;
        creator_account.fee_collected = creator_account.fee_collected
            .checked_add(creator_fee).ok_or(error!(BetError::MathOverflow))?;
    }

    let market = &ctx.accounts.market;
    emit!(events::WinningsClaimed {
        market_id: market.market_id.clone(),
        bettor: ctx.accounts.bet.bettor,
        nonce: ctx.accounts.bet.nonce,
        payout: actual_payout,
        fee,
        creator_fee,
    });

    Ok(())
}
