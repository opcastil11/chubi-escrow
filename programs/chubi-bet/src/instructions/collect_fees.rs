use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::BetError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct CollectBetFees<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: Vault PDA sends the fees.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    /// CHECK: Must match market.fee_recipient.
    #[account(
        mut,
        constraint = fee_recipient.key() == market.fee_recipient @ BetError::FeeRecipientMismatch,
    )]
    pub fee_recipient: UncheckedAccount<'info>,

    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CollectBetFees>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let fees = market.protocol_fee_collected;
    require!(fees > 0, BetError::NoFees);

    // Never dip into stakes still owed to unclaimed winners: cap at whatever
    // the vault holds above its rent-exempt minimum.
    let vault_balance = ctx.accounts.vault.lamports();
    let min_rent = Rent::get()?.minimum_balance(0);
    let available = vault_balance.saturating_sub(min_rent);
    let actual_fees = fees.min(available);
    require!(actual_fees > 0, BetError::NoFees);

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
                to: ctx.accounts.fee_recipient.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual_fees,
    )?;

    market.protocol_fee_collected = market.protocol_fee_collected
        .saturating_sub(actual_fees);

    emit!(events::BetFeesCollected {
        market_id: market.market_id.clone(),
        amount: actual_fees,
        recipient: ctx.accounts.fee_recipient.key(),
    });

    Ok(())
}
