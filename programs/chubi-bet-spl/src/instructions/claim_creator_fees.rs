use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
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

    /// Vault token account pays the commission. Owned by the market PDA.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
        token::mint = mint,
        token::authority = market,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    /// The SPL mint the market settles in.
    pub mint: InterfaceAccount<'info, Mint>,

    /// Creator's token account, the destination of the commission.
    #[account(
        mut,
        token::mint = mint,
        token::authority = creator,
    )]
    pub creator_token: InterfaceAccount<'info, TokenAccount>,

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

    pub token_program: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimBetCreatorFees>) -> Result<()> {
    let amount = ctx.accounts.creator_account.fee_collected;
    require!(amount > 0, BetError::NoCreatorFees);

    let available = ctx.accounts.vault.amount;
    let actual = amount.min(available);
    require!(actual > 0, BetError::NoCreatorFees);

    // Vault token authority is the MARKET PDA → sign with the MARKET seeds.
    let market_id_bytes = ctx.accounts.market.market_id.as_bytes().to_vec();
    let market_bump = ctx.accounts.market.bump;
    let market_seeds: &[&[u8]] = &[
        b"market",
        market_id_bytes.as_ref(),
        std::slice::from_ref(&market_bump),
    ];

    token_interface::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.creator_token.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            &[market_seeds],
        ),
        actual,
        ctx.accounts.mint.decimals,
    )?;

    let creator_account = &mut ctx.accounts.creator_account;
    creator_account.fee_collected = creator_account.fee_collected
        .checked_sub(actual).ok_or(error!(BetError::MathOverflow))?;
    creator_account.fee_claimed = creator_account.fee_claimed
        .checked_add(actual).ok_or(error!(BetError::MathOverflow))?;

    emit!(events::BetCreatorFeesClaimed {
        market_id: ctx.accounts.market.market_id.clone(),
        creator: ctx.accounts.creator_account.creator,
        amount: actual,
    });

    Ok(())
}
