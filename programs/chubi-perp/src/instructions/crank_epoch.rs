use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::PerpError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct CrankEpoch<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Vault PDA — pays the cranker rebate (subset of funding lamports).
    #[account(
        mut,
        seeds = [b"perp_vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    /// Whoever calls. Receives a small rebate proportional to the funding moved
    /// this epoch — makes the permissionless crank self-sustaining instead of
    /// relying on the authority to keep its wallet funded.
    #[account(mut)]
    pub cranker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CrankEpoch>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(!market.is_closed, PerpError::MarketClosed);
    require!(
        now - market.last_epoch_at >= EPOCH_SECS,
        PerpError::EpochNotElapsed
    );

    // compute_funding errors if pools are empty / balanced — those are valid
    // states that just don't produce funding. Bubble the error so the caller
    // can stop polling for a quiet market.
    let (winner_side, loser_side, funding, imbalance_bps) = math::compute_funding(&market.pools)?;

    // Cranker rebate: a fraction of `funding` is diverted out of the vault to
    // the caller. The loser pool still drains the full `funding` (preserves
    // the funding-pressure semantic), but the winner pool only gets the net.
    let cranker_rebate = (funding as u128)
        .checked_mul(CRANKER_REBATE_BPS as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))? as u64;

    // Clamp rebate by vault headroom so we never go below rent-exempt minimum.
    let vault_balance = ctx.accounts.vault.lamports();
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(0);
    let vault_available = vault_balance.saturating_sub(min_rent);
    let actual_rebate = cranker_rebate.min(vault_available);
    let funding_to_winner = funding.checked_sub(actual_rebate).ok_or(error!(PerpError::MathOverflow))?;

    market.pools[loser_side as usize] = market.pools[loser_side as usize]
        .checked_sub(funding).ok_or(error!(PerpError::MathOverflow))?;
    market.pools[winner_side as usize] = market.pools[winner_side as usize]
        .checked_add(funding_to_winner).ok_or(error!(PerpError::MathOverflow))?;
    // weighted_pools intentionally NOT touched. Holders' weighted_share stays
    // constant; only the lamports denominator (pools[]) changes, so each
    // holder's exit value moves with funding flow proportional to their weight.

    if actual_rebate > 0 {
        let market_key = market.key();
        let vault_seeds = &[
            b"perp_vault" as &[u8],
            market_key.as_ref(),
            &[market.vault_bump],
        ];
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.cranker.to_account_info(),
                },
                &[vault_seeds],
            ),
            actual_rebate,
        )?;
    }

    market.cumulative_funding_paid = market.cumulative_funding_paid
        .checked_add(funding).ok_or(error!(PerpError::MathOverflow))?;
    market.last_epoch_at = now;

    emit!(events::EpochCranked {
        market_id: market.market_id.clone(),
        winner_side,
        funding_lamports: funding,
        imbalance_bps,
        cranker: ctx.accounts.cranker.key(),
        timestamp: now,
        cranker_rebate: actual_rebate,
    });

    Ok(())
}
