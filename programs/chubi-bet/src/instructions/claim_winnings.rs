use anchor_lang::prelude::*;
use anchor_lang::system_program;
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

    /// CHECK: Vault PDA pays the winnings.
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

    #[account(
        mut,
        seeds = [b"creator", market.key().as_ref()],
        bump = creator_account.bump,
        constraint = creator_account.market == market.key() @ BetError::CreatorMarketMismatch,
    )]
    pub creator_account: Account<'info, BetCreator>,

    #[account(mut)]
    pub bettor: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimWinnings>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let bet = &mut ctx.accounts.bet;

    require!(market.status == BetStatus::Resolved, BetError::MarketNotResolved);
    require!(!bet.claimed, BetError::AlreadyClaimed);

    // winner is 1-indexed, bet.side is 0-indexed
    let winner_idx = market.winner.checked_sub(1).ok_or(error!(BetError::InvalidWinner))?;
    // Losers get nothing — that is the whole of parimutuel. They can still
    // recover their rent with `close_bet`.
    require!(bet.side == winner_idx, BetError::NoPayout);

    let total_pool = math::sum_pools(&market.pools, market.num_sides);
    let winner_pool = market.pools[winner_idx as usize];

    // Anonymous / house markets carry a side-car with creator = default and
    // pay no commission.
    let creator_fee_bps = if ctx.accounts.creator_account.creator != Pubkey::default() {
        CREATOR_FEE_BPS
    } else {
        0
    };

    let (payout, fee, creator_fee) =
        math::parimutuel_payout(bet.amount, winner_pool, total_pool, creator_fee_bps)?;
    require!(payout > 0, BetError::NoPayout);

    // Keep the vault rent-exempt: truncation dust and the vault's own rent
    // must survive the last claim.
    let vault_balance = ctx.accounts.vault.lamports();
    let min_rent = Rent::get()?.minimum_balance(0); // SystemAccount = 0 data bytes
    let available = vault_balance.saturating_sub(min_rent);
    let actual_payout = payout.min(available);
    require!(actual_payout > 0, BetError::InsufficientVault);

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
        actual_payout,
    )?;

    bet.claimed = true;
    bet.payout_amount = actual_payout;
    market.total_paid = market.total_paid
        .checked_add(actual_payout).ok_or(error!(BetError::MathOverflow))?;
    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_add(fee).ok_or(error!(BetError::MathOverflow))?;

    if creator_fee > 0 {
        let creator_account = &mut ctx.accounts.creator_account;
        creator_account.fee_collected = creator_account.fee_collected
            .checked_add(creator_fee).ok_or(error!(BetError::MathOverflow))?;
    }

    emit!(events::WinningsClaimed {
        market_id: market.market_id.clone(),
        bettor: bet.bettor,
        nonce: bet.nonce,
        payout: actual_payout,
        fee,
        creator_fee,
    });

    Ok(())
}
