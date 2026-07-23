use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::BetError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
#[instruction(side: u8, amount: u64, max_odds: u64, nonce: u64)]
pub struct PlaceBet<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: Vault PDA receives the stake.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    // Last seed is the CLIENT-supplied `nonce`. A market-global counter would
    // serialize the market: two bettors reading the same count derive the same
    // address and the second transaction fails. `bettor` is already in the
    // seed, so a client nonce only collides with the same wallet reusing one.
    #[account(
        init,
        payer = bettor,
        space = 8 + Bet::INIT_SPACE,
        seeds = [
            b"bet",
            market.key().as_ref(),
            bettor.key().as_ref(),
            nonce.to_le_bytes().as_ref(),
        ],
        bump,
    )]
    pub bet: Account<'info, Bet>,

    #[account(mut)]
    pub bettor: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<PlaceBet>,
    side: u8,
    amount: u64,
    max_odds: u64,
    nonce: u64,
) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(market.status == BetStatus::Open, BetError::MarketNotOpen);
    require!((side as usize) < market.num_sides as usize, BetError::InvalidSide);
    require!(amount >= MIN_BET_LAMPORTS, BetError::BetTooSmall);
    require!(now < market.created_at + market.close_duration, BetError::BettingClosed);

    // ── Price after this stake lands ──
    // In a parimutuel pool the bettor's own money moves the price, so the
    // meaningful number is the post-bet share: it is exactly the reciprocal of
    // the payout multiple they will be settled at if nothing else changes.
    let s = side as usize;
    let new_side_pool = market.pools[s]
        .checked_add(amount).ok_or(error!(BetError::MathOverflow))?;
    let new_total = market.total_staked
        .checked_add(amount).ok_or(error!(BetError::MathOverflow))?;
    let entry_odds = math::implied_odds(new_side_pool, new_total);

    // ── Slippage guard ──
    // `max_odds = 0` disables it. Callers should pass the price they showed
    // the user plus a small tolerance. This only protects against drift
    // between click and landing; later bets keep moving the pool, which is
    // inherent to parimutuel and cannot be guarded here.
    if max_odds > 0 {
        require!(entry_odds <= max_odds, BetError::OddsSlippage);
    }

    // ── Transfer the stake into the vault ──
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.bettor.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        amount,
    )?;

    let bet = &mut ctx.accounts.bet;
    bet.bump = ctx.bumps.bet;
    bet.market = market.key();
    bet.bettor = ctx.accounts.bettor.key();
    bet.side = side;
    bet.amount = amount;
    bet.payout_amount = 0;
    bet.nonce = nonce;
    bet.claimed = false;
    bet.created_at = now;
    bet.entry_odds = entry_odds;

    market.pools[s] = new_side_pool;
    market.total_staked = new_total;
    market.side_bet_counts[s] = market.side_bet_counts[s]
        .checked_add(1).ok_or(error!(BetError::MathOverflow))?;
    market.bet_count = market.bet_count
        .checked_add(1).ok_or(error!(BetError::MathOverflow))?;

    emit!(events::BetPlaced {
        market_id: market.market_id.clone(),
        bettor: bet.bettor,
        side,
        amount,
        entry_odds,
        nonce,
        timestamp: now,
    });

    Ok(())
}
