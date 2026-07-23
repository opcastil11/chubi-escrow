use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::errors::BetError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
pub struct ClaimBetCreatorFees<'info> {
    #[account(
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: Vault PDA pays the commission.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        seeds = [b"creator", market.key().as_ref()],
        bump = creator_account.bump,
        constraint = creator_account.market == market.key() @ BetError::CreatorMarketMismatch,
        constraint = creator_account.creator == creator.key() @ BetError::NotCreator,
    )]
    pub creator_account: Account<'info, BetCreator>,

    #[account(mut)]
    pub creator: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimBetCreatorFees>) -> Result<()> {
    let creator_account = &mut ctx.accounts.creator_account;
    let amount = creator_account.fee_collected;
    require!(amount > 0, BetError::NoCreatorFees);

    let vault_balance = ctx.accounts.vault.lamports();
    let min_rent = Rent::get()?.minimum_balance(0);
    let available = vault_balance.saturating_sub(min_rent);
    let actual = amount.min(available);
    require!(actual > 0, BetError::NoCreatorFees);

    let market = &ctx.accounts.market;
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
                to: ctx.accounts.creator.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual,
    )?;

    creator_account.fee_collected = creator_account.fee_collected
        .checked_sub(actual).ok_or(error!(BetError::MathOverflow))?;
    creator_account.fee_claimed = creator_account.fee_claimed
        .checked_add(actual).ok_or(error!(BetError::MathOverflow))?;

    emit!(events::BetCreatorFeesClaimed {
        market_id: market.market_id.clone(),
        creator: creator_account.creator,
        amount: actual,
    });

    Ok(())
}
