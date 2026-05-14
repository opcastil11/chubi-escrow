use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::PerpError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct CollectPerpFees<'info> {
    #[account(
        mut,
        seeds = [b"perp_market", market.market_id.as_bytes()],
        bump = market.bump,
        constraint = market.authority == authority.key() @ PerpError::NotPerpetualAdmin,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Vault PDA — pays out accumulated protocol fees.
    #[account(
        mut,
        seeds = [b"perp_vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    /// CHECK: Receives the swept fees. Validated against `market.fee_recipient`.
    #[account(mut, address = market.fee_recipient @ PerpError::NotPerpetualAdmin)]
    pub recipient: SystemAccount<'info>,

    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CollectPerpFees>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let amount = market.protocol_fee_collected;
    require!(amount > 0, PerpError::NoProtocolFees);

    let vault_balance = ctx.accounts.vault.lamports();
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(0);
    let available = vault_balance.saturating_sub(min_rent);
    let actual = amount.min(available);
    require!(actual > 0, PerpError::InsufficientVault);

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
                to: ctx.accounts.recipient.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual,
    )?;

    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_sub(actual).ok_or(error!(PerpError::MathOverflow))?;

    emit!(events::PerpProtocolFeesCollected {
        market_id: market.market_id.clone(),
        recipient: ctx.accounts.recipient.key(),
        amount: actual,
    });

    Ok(())
}
