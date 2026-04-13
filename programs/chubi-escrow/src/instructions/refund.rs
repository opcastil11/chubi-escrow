use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::ChubiError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct Refund<'info> {
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

pub fn handler(ctx: Context<Refund>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let position = &mut ctx.accounts.position;

    require!(market.status == MarketStatus::Invalid, ChubiError::MarketNotInvalid);
    require!(!position.claimed, ChubiError::AlreadyClaimed);
    require!(!position.withdrawn, ChubiError::AlreadyWithdrawn);

    let refund_amount = position.amount;

    // Transfer full deposit from vault → maker
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
        refund_amount,
    )?;

    position.claimed = true;
    position.payout_amount = refund_amount;
    market.total_claimed = market.total_claimed
        .checked_add(refund_amount).ok_or(error!(ChubiError::MathOverflow))?;

    emit!(events::Refunded {
        market_id: market.market_id.clone(),
        maker: position.maker,
        nonce: position.nonce,
        amount: refund_amount,
    });

    Ok(())
}
