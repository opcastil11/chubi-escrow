use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::ChubiError;
use crate::events;
use crate::state::*;

#[derive(Accounts)]
#[instruction(market_id: String)]
pub struct CreateMarket<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + MarketState::INIT_SPACE,
        seeds = [b"market", market_id.as_bytes()],
        bump,
    )]
    pub market: Account<'info, MarketState>,

    /// CHECK: Vault PDA — just holds lamports, no data.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<CreateMarket>,
    market_id: String,
    resolution_duration: i64,
    num_sides: u8,
    allow_withdrawal: bool,
    enable_lockout: bool,
    fee_recipient: Pubkey,
) -> Result<()> {
    require!(market_id.len() <= MAX_MARKET_ID_LEN, ChubiError::MarketIdTooLong);
    require!(
        resolution_duration >= MIN_DURATION_SECS && resolution_duration <= MAX_DURATION_SECS,
        ChubiError::InvalidDuration
    );
    require!(num_sides >= 2 && num_sides as usize <= MAX_SIDES, ChubiError::InvalidNumSides);

    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;

    market.bump = ctx.bumps.market;
    market.vault_bump = ctx.bumps.vault;
    market.authority = ctx.accounts.authority.key();
    market.market_id = market_id.clone();
    market.status = MarketStatus::Open;
    market.winner = 0;
    market.num_sides = num_sides;
    market.created_at = now;
    market.resolution_duration = resolution_duration;
    market.resolved_at = 0;
    market.total_deposited = 0;
    market.total_claimed = 0;
    market.position_count = 0;
    market.pools = [0; MAX_SIDES];
    market.weighted_pools = [0; MAX_SIDES];
    market.side_position_counts = [0; MAX_SIDES];
    market.first_deposit_at = [0; MAX_SIDES];
    market.penalty_pool = 0;
    market.cumulative_twd_0 = 0;
    market.cumulative_time = 0;
    market.last_snapshot_at = now;
    market.has_both_sides = false;
    market.winner_payout_share = 0;
    market.twd_0 = 0;
    market.allow_withdrawal = allow_withdrawal;
    market.enable_lockout = enable_lockout;
    market.protocol_fee_collected = 0;
    market.fee_recipient = fee_recipient;

    emit!(events::MarketCreated {
        market_id,
        authority: market.authority,
        num_sides,
        resolution_duration,
        allow_withdrawal,
        enable_lockout,
    });

    Ok(())
}
