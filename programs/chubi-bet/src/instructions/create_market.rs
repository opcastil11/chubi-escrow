use anchor_lang::prelude::*;
use anchor_lang::system_program;
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

    /// CHECK: Vault PDA — holds staked lamports, no data. Seeded with its
    /// rent-exempt minimum by the handler; see the comment there.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump,
    )]
    pub vault: SystemAccount<'info>,

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
    // Dispute layer — dormant when dispute_window == 0.
    market.dispute_window = dispute_window;
    market.proposed_winner = 0;
    market.proposed_at = 0;
    market.disputer = Pubkey::default();
    market.dispute_bond = 0;

    // ── Seed the vault with its own rent-exempt minimum ──
    //
    // Parimutuel pays the ENTIRE pot out to winners, so unlike chubi-escrow
    // (where the losers' share always leaves a cushion) the vault's balance
    // converges on just the accrued fees — which can be far below the
    // rent-exempt minimum. Without a seed, two things break: the last winner's
    // payout gets silently clipped by the `available` cap in claim_winnings,
    // and `collect_fees` then finds nothing to sweep.
    //
    // The SystemProgram also refuses a transfer that would leave a source with
    // a non-zero balance below the rent minimum, so the vault cannot simply be
    // allowed to drain either. The authority funds ~0.00089 SOL here and that
    // lamport floor stays put for the life of the market; every accounting
    // path above it then works on the pot alone.
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.authority.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        Rent::get()?.minimum_balance(0),
    )?;

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
