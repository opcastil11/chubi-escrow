use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
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

    /// Vault token account sends the fees. Owned by the market PDA.
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

    /// Fee recipient token account. Must belong to `market.fee_recipient`.
    #[account(
        mut,
        token::mint = mint,
        token::authority = market.fee_recipient,
    )]
    pub fee_recipient_token: InterfaceAccount<'info, TokenAccount>,

    pub authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CollectBetFees>) -> Result<()> {
    let fees = ctx.accounts.market.protocol_fee_collected;
    require!(fees > 0, BetError::NoFees);

    // Never dip into stakes still owed to unclaimed winners: cap at whatever
    // token balance the vault holds.
    let available = ctx.accounts.vault.amount;
    let actual_fees = fees.min(available);
    require!(actual_fees > 0, BetError::NoFees);

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
                to: ctx.accounts.fee_recipient_token.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            &[market_seeds],
        ),
        actual_fees,
        ctx.accounts.mint.decimals,
    )?;

    let market = &mut ctx.accounts.market;
    market.protocol_fee_collected = market.protocol_fee_collected
        .saturating_sub(actual_fees);

    let recipient = market.fee_recipient;
    emit!(events::BetFeesCollected {
        market_id: market.market_id.clone(),
        amount: actual_fees,
        recipient,
    });

    Ok(())
}
