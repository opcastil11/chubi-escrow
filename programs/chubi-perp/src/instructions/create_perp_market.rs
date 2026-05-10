use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::PerpError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
#[instruction(market_id: String)]
pub struct CreatePerpMarket<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + PerpMarketState::INIT_SPACE,
        seeds = [b"perp_market", market_id.as_bytes()],
        bump,
    )]
    pub market: Account<'info, PerpMarketState>,

    /// CHECK: Vault PDA — holds lamports, no data.
    #[account(
        mut,
        seeds = [b"perp_vault", market.key().as_ref()],
        bump,
    )]
    pub vault: SystemAccount<'info>,

    /// Creator side-car — accumulates the 0.5% commission.
    #[account(
        init,
        payer = authority,
        space = 8 + CreatorAccount::INIT_SPACE,
        seeds = [b"perp_creator", market.key().as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, CreatorAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<CreatePerpMarket>,
    market_id: String,
    fee_recipient: Pubkey,
    creator: Pubkey,
) -> Result<()> {
    require!(market_id.len() <= MAX_MARKET_ID_LEN, PerpError::MarketIdTooLong);

    // Admin gate — `creator` is the wallet attribution from the backend; only
    // the hardcoded admin pubkeys can launch perpetuals.
    require!(
        PERPETUAL_ADMINS.contains(&creator),
        PerpError::NotPerpetualAdmin
    );

    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;

    market.bump = ctx.bumps.market;
    market.vault_bump = ctx.bumps.vault;
    market.authority = ctx.accounts.authority.key();
    market.market_id = market_id.clone();
    market.num_sides = NUM_SIDES;
    market.is_closed = false;
    market.created_at = now;
    market.last_epoch_at = now;
    market.pools = [0; 2];
    market.weighted_pools = [0u128; 2];
    market.side_position_counts = [0; 2];
    market.first_deposit_at = [0; 2];
    market.total_deposited = 0;
    market.total_exited = 0;
    market.position_count = 0;
    market.cumulative_funding_paid = 0;
    market.protocol_fee_collected = 0;
    market.fee_recipient = fee_recipient;

    let creator_account = &mut ctx.accounts.creator_account;
    creator_account.bump = ctx.bumps.creator_account;
    creator_account.market = market.key();
    creator_account.creator = creator;
    creator_account.fee_collected = 0;
    creator_account.fee_claimed = 0;

    emit!(events::PerpMarketCreated {
        market_id,
        authority: market.authority,
        creator,
    });

    Ok(())
}
