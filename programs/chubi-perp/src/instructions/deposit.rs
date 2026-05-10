use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::PerpError;
use crate::events;
use crate::math;
use crate::state::*;

#[derive(Accounts)]
pub struct PerpDeposit<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Vault PDA receives SOL.
    #[account(
        mut,
        seeds = [b"perp_vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        init,
        payer = maker,
        space = 8 + PerpPositionState::INIT_SPACE,
        seeds = [
            b"perp_position",
            market.key().as_ref(),
            maker.key().as_ref(),
            market.position_count.to_le_bytes().as_ref(),
        ],
        bump,
    )]
    pub position: Account<'info, PerpPositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<PerpDeposit>, side: u8, amount: u64) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(!market.is_closed, PerpError::MarketClosed);
    require!((side as usize) < market.num_sides as usize, PerpError::InvalidSide);
    require!(amount >= MIN_DEPOSIT_LAMPORTS, PerpError::DepositTooSmall);

    // Entry weight is computed against the market's age — late entrants get
    // a smaller share of any future funding flow.
    let elapsed_since_market = now - market.created_at;
    let entry_weight = math::compute_entry_weight(elapsed_since_market)?;

    // Transfer SOL: maker → vault.
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.maker.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        amount,
    )?;

    // Initialize position.
    let position = &mut ctx.accounts.position;
    position.bump = ctx.bumps.position;
    position.market = market.key();
    position.maker = ctx.accounts.maker.key();
    position.side = side;
    position.amount = amount;
    position.nonce = market.position_count;
    position.created_at = now;
    position.entry_weight = entry_weight;
    position.exited = false;
    position.exit_payout = 0;

    // Update market state.
    let s = side as usize;
    market.pools[s] = market.pools[s]
        .checked_add(amount).ok_or(error!(PerpError::MathOverflow))?;
    let weighted_contrib = (amount as u128)
        .checked_mul(entry_weight as u128).ok_or(error!(PerpError::MathOverflow))?;
    market.weighted_pools[s] = market.weighted_pools[s]
        .checked_add(weighted_contrib).ok_or(error!(PerpError::MathOverflow))?;
    market.side_position_counts[s] = market.side_position_counts[s]
        .checked_add(1).ok_or(error!(PerpError::MathOverflow))?;
    market.total_deposited = market.total_deposited
        .checked_add(amount).ok_or(error!(PerpError::MathOverflow))?;
    market.position_count = market.position_count
        .checked_add(1).ok_or(error!(PerpError::MathOverflow))?;
    if market.first_deposit_at[s] == 0 {
        market.first_deposit_at[s] = now;
    }

    emit!(events::PerpDeposited {
        market_id: market.market_id.clone(),
        maker: position.maker,
        side,
        amount,
        entry_weight,
        nonce: position.nonce,
        timestamp: now,
    });

    Ok(())
}
