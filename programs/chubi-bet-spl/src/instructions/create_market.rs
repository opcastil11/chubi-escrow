use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use crate::constants::*;
use crate::errors::BetError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
#[instruction(market_id: String)]
pub struct CreateBetMarket<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + BetMarket::INIT_SPACE,
        seeds = [b"market", market_id.as_bytes()],
        bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// Vault token account — holds staked tokens. Owned (authority) by the
    /// market PDA, so payouts move via a signed CPI with the MARKET seeds. A
    /// token account is rent-exempt on `init` with no seeding needed.
    #[account(
        init,
        payer = authority,
        seeds = [b"vault", market.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = market,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    /// The SPL mint this market settles in (e.g. USDC).
    pub mint: InterfaceAccount<'info, Mint>,

    /// Creator side-car. Always initialized so every market has the same
    /// account layout; `creator = Pubkey::default()` marks a house market that
    /// pays no commission. The authority pays the rent because the creator
    /// does not sign this transaction.
    #[account(
        init,
        payer = authority,
        space = 8 + BetCreator::INIT_SPACE,
        seeds = [b"creator", market.key().as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, BetCreator>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<CreateBetMarket>,
    market_id: String,
    close_duration: i64,
    num_sides: u8,
    fee_recipient: Pubkey,
    creator: Pubkey,
    dispute_window: i64,
) -> Result<()> {
    require!(market_id.len() <= MAX_MARKET_ID_LEN, BetError::MarketIdTooLong);
    require!(
        close_duration >= MIN_DURATION_SECS && close_duration <= MAX_DURATION_SECS,
        BetError::InvalidDuration
    );
    require!(num_sides >= 2 && num_sides as usize <= MAX_SIDES, BetError::InvalidNumSides);
    require!(
        dispute_window >= 0 && dispute_window <= MAX_DISPUTE_WINDOW_SECS,
        BetError::InvalidDisputeWindow
    );

    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;

    market.bump = ctx.bumps.market;
    market.vault_bump = ctx.bumps.vault;
    market.authority = ctx.accounts.authority.key();
    market.market_id = market_id.clone();
    market.status = BetStatus::Open;
    market.winner = 0;
    market.num_sides = num_sides;
    market.created_at = now;
    market.close_duration = close_duration;
    market.resolved_at = 0;
    market.total_staked = 0;
    market.total_paid = 0;
    market.bet_count = 0;
    market.pools = [0; MAX_SIDES];
    market.side_bet_counts = [0; MAX_SIDES];
    market.protocol_fee_collected = 0;
    market.fee_recipient = fee_recipient;
    market.mint = ctx.accounts.mint.key();
    // Dispute layer — dormant when dispute_window == 0.
    market.dispute_window = dispute_window;
    market.proposed_winner = 0;
    market.proposed_at = 0;
    market.disputer = Pubkey::default();
    market.dispute_bond = 0;

    // No rent seeding: the vault is a token account, rent-exempt on `init`. Its
    // token balance starts at 0 and every accounting path works on that balance
    // (`vault.amount`), so there is no lamport floor to preserve.

    let creator_account = &mut ctx.accounts.creator_account;
    creator_account.bump = ctx.bumps.creator_account;
    creator_account.market = market.key();
    creator_account.creator = creator;
    creator_account.fee_collected = 0;
    creator_account.fee_claimed = 0;

    emit!(events::BetMarketCreated {
        market_id,
        authority: market.authority,
        num_sides,
        close_duration,
        creator,
    });

    Ok(())
}
