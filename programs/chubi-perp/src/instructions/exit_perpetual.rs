use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::PerpError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct ExitPerpetual<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Vault PDA sends SOL.
    #[account(
        mut,
        seeds = [b"perp_vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        close = maker,
        constraint = position.market == market.key() @ PerpError::PositionMarketMismatch,
        constraint = position.maker == maker.key() @ PerpError::NotYourPosition,
    )]
    pub position: Account<'info, PerpPositionState>,

    /// Optional CreatorAccount side-car. If active (creator != Pubkey::default),
    /// 0.5% of profit accumulates here for later sweep by `claim_creator_fees`.
    #[account(
        mut,
        seeds = [b"perp_creator", market.key().as_ref()],
        bump = creator_account.bump,
    )]
    pub creator_account: Account<'info, CreatorAccount>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ExitPerpetual>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let position = &ctx.accounts.position;
    // No AlreadyExited check needed: `close = maker` on the position account
    // means a re-entry on the same PDA fails Anchor account deserialization.

    // Compute current fair value at the existing pool snapshot. Funding is
    // already baked into pools[] by prior crank_epochs.
    let gross_value = math::compute_exit_value(
        position.amount,
        position.entry_weight,
        position.side,
        &market.pools,
        &market.weighted_pools,
    )?;
    require!(gross_value > 0, PerpError::InsufficientVault);

    // Fees on profit only (matches chubi-escrow timed-market accounting).
    let profit = gross_value.saturating_sub(position.amount);
    let creator_fee_bps = if creator_account_active(&ctx.accounts.creator_account) { CREATOR_FEE_BPS } else { 0 };
    let protocol_fee = (profit as u128)
        .checked_mul(PROTOCOL_FEE_BPS as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))? as u64;
    let creator_fee = (profit as u128)
        .checked_mul(creator_fee_bps as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))? as u64;
    let net_payout = gross_value
        .checked_sub(protocol_fee).ok_or(error!(PerpError::MathOverflow))?
        .checked_sub(creator_fee).ok_or(error!(PerpError::MathOverflow))?;

    // Cap by available vault balance (rent-exempt minimum stays).
    let vault_balance = ctx.accounts.vault.lamports();
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(0);
    let available = vault_balance.saturating_sub(min_rent);
    let actual_payout = net_payout.min(available);
    require!(actual_payout > 0, PerpError::InsufficientVault);

    // If the vault was short and we capped, scale gross + fees by the same
    // ratio so pools[] and the fee accumulators stay consistent with what was
    // actually moved. In a healthy vault `actual_payout == net_payout` and
    // this branch is a no-op.
    let (paid_gross, paid_protocol_fee, paid_creator_fee) = if actual_payout < net_payout {
        let scale = actual_payout as u128;
        let denom = net_payout as u128;
        let g = (gross_value as u128)
            .checked_mul(scale).ok_or(error!(PerpError::MathOverflow))?
            .checked_div(denom).ok_or(error!(PerpError::MathOverflow))? as u64;
        let pf = (protocol_fee as u128)
            .checked_mul(scale).ok_or(error!(PerpError::MathOverflow))?
            .checked_div(denom).ok_or(error!(PerpError::MathOverflow))? as u64;
        let cf = (creator_fee as u128)
            .checked_mul(scale).ok_or(error!(PerpError::MathOverflow))?
            .checked_div(denom).ok_or(error!(PerpError::MathOverflow))? as u64;
        (g, pf, cf)
    } else {
        (gross_value, protocol_fee, creator_fee)
    };

    // Vault → maker.
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
                to: ctx.accounts.maker.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual_payout,
    )?;

    // Update market accounting. Pool & fees use the scaled `paid_*` values so
    // book-keeping always matches lamports actually moved. weighted_pools is
    // decremented in full because the position is being closed regardless.
    let s = position.side as usize;
    market.pools[s] = market.pools[s].saturating_sub(paid_gross);
    let weighted_contrib = (position.amount as u128)
        .checked_mul(position.entry_weight as u128).ok_or(error!(PerpError::MathOverflow))?;
    market.weighted_pools[s] = market.weighted_pools[s].saturating_sub(weighted_contrib);
    market.side_position_counts[s] = market.side_position_counts[s].saturating_sub(1);
    market.total_exited = market.total_exited
        .checked_add(actual_payout).ok_or(error!(PerpError::MathOverflow))?;
    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_add(paid_protocol_fee).ok_or(error!(PerpError::MathOverflow))?;

    // Accumulate creator fee on side-car.
    if paid_creator_fee > 0 {
        let creator_account = &mut ctx.accounts.creator_account;
        creator_account.fee_collected = creator_account.fee_collected
            .checked_add(paid_creator_fee).ok_or(error!(PerpError::MathOverflow))?;
    }

    emit!(events::PerpExited {
        market_id: market.market_id.clone(),
        maker: position.maker,
        nonce: position.nonce,
        gross_value: paid_gross,
        principal: position.amount,
        protocol_fee: paid_protocol_fee,
        creator_fee: paid_creator_fee,
        net_payout: actual_payout,
    });

    Ok(())
}

fn creator_account_active(acct: &CreatorAccount) -> bool {
    acct.creator != Pubkey::default()
}
