use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::ChubiError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct ClaimPayout<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketState>,

    /// CHECK: Vault PDA sends SOL.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
        constraint = position.maker == maker.key() @ ChubiError::NotYourPosition,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimPayout>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let position = &mut ctx.accounts.position;

    require!(market.status == MarketStatus::Resolved, ChubiError::MarketNotResolved);
    require!(!position.claimed, ChubiError::AlreadyClaimed);
    require!(!position.withdrawn, ChubiError::AlreadyWithdrawn);

    // Winner is 1-indexed, position.side is 0-indexed
    let winner_idx = market.winner.checked_sub(1).ok_or(error!(ChubiError::InvalidWinner))?;
    let is_winner = position.side == winner_idx;

    // Compute total distributable: active pools + penalty pool
    let total_distributable = math::sum_pools(&market.pools, market.num_sides)
        .checked_add(market.penalty_pool).ok_or(error!(ChubiError::MathOverflow))?;

    let (payout, fee) = if is_winner {
        math::compute_winner_payout(
            position.amount,
            position.entry_weight,
            winner_idx,
            &market.pools,
            &market.weighted_pools,
            market.winner_payout_share,
            total_distributable,
        )?
    } else {
        let loser_payout = math::compute_loser_payout(
            position.amount,
            position.entry_weight,
            position.side,
            &market.weighted_pools,
            market.winner_payout_share,
            total_distributable,
        )?;
        (loser_payout, 0u64)
    };

    require!(payout > 0, ChubiError::NoPayout);

    // Cap payout to vault balance (safety)
    let vault_balance = ctx.accounts.vault.lamports();
    let actual_payout = payout.min(vault_balance);
    require!(actual_payout > 0, ChubiError::InsufficientVault);

    // Transfer SOL from vault → maker (vault PDA signs)
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
                to: ctx.accounts.maker.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual_payout,
    )?;

    // Update state
    position.claimed = true;
    position.payout_amount = actual_payout;
    market.total_claimed = market.total_claimed
        .checked_add(actual_payout).ok_or(error!(ChubiError::MathOverflow))?;
    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_add(fee).ok_or(error!(ChubiError::MathOverflow))?;

    emit!(events::PayoutClaimed {
        market_id: market.market_id.clone(),
        maker: position.maker,
        nonce: position.nonce,
        payout: actual_payout,
        fee,
        is_winner,
    });

    Ok(())
}
