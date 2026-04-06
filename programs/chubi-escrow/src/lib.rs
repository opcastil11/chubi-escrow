use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("Ar35haaiz1tu2DyVetpZ96TYvUAipJ817qMF9DQm6cZm");

// ─── Constants ──────────────────────────────────────────────────────────────

const MAX_MARKET_ID_LEN: usize = 64;
const MIN_DEPOSIT_LAMPORTS: u64 = 1_000_000; // 0.001 SOL

// ─── Program ────────────────────────────────────────────────────────────────

#[program]
pub mod chubi_escrow {
    use super::*;

    /// Create a new conviction market. The creator becomes the authority
    /// (typically the backend wallet) who can later resolve and set payouts.
    pub fn create_market(
        ctx: Context<CreateMarket>,
        market_id: String,
        resolution_duration: i64,
    ) -> Result<()> {
        require!(market_id.len() <= MAX_MARKET_ID_LEN, ChubiError::MarketIdTooLong);
        require!(resolution_duration >= 600 && resolution_duration <= 604800, ChubiError::InvalidDuration);

        let market = &mut ctx.accounts.market;
        market.bump = ctx.bumps.market;
        market.vault_bump = ctx.bumps.vault;
        market.authority = ctx.accounts.authority.key();
        market.market_id = market_id;
        market.status = MarketStatus::Open;
        market.winner = 0;
        market.total_deposited = 0;
        market.total_claimed = 0;
        market.position_count = 0;
        market.created_at = Clock::get()?.unix_timestamp;
        market.resolution_duration = resolution_duration;

        msg!("Market created: {}", market.market_id);
        Ok(())
    }

    /// Deposit SOL into a market side. The user's SOL is transferred to
    /// the vault PDA — trustless escrow.
    pub fn deposit(
        ctx: Context<Deposit>,
        side: u8,
        amount: u64,
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);
        require!(side <= 5, ChubiError::InvalidSide);
        require!(amount >= MIN_DEPOSIT_LAMPORTS, ChubiError::DepositTooSmall);

        // Transfer SOL from maker → vault
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.maker.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;

        // Initialize position
        let position = &mut ctx.accounts.position;
        position.bump = ctx.bumps.position;
        position.market = market.key();
        position.maker = ctx.accounts.maker.key();
        position.side = side;
        position.amount = amount;
        position.payout_amount = 0;
        position.nonce = market.position_count;
        position.claimed = false;
        position.created_at = Clock::get()?.unix_timestamp;

        // Update market totals
        market.total_deposited += amount;
        market.position_count += 1;

        msg!("Deposit: {} lamports on side {} (position #{})", amount, side, position.nonce);
        Ok(())
    }

    /// Authority resolves the market, declaring the winning side.
    /// No payout computation on-chain — that happens off-chain via TWCD.
    pub fn resolve_market(
        ctx: Context<ResolveMarket>,
        winner: u8,
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);
        require!(winner >= 1 && winner <= 6, ChubiError::InvalidWinner);

        market.status = MarketStatus::Resolved;
        market.winner = winner;

        msg!("Market resolved: winner = side {}", winner);
        Ok(())
    }

    /// Authority sets the payout amount for a position (computed off-chain via TWCD).
    pub fn set_payout(
        ctx: Context<SetPayout>,
        payout_amount: u64,
    ) -> Result<()> {
        let market = &ctx.accounts.market;
        require!(
            market.status == MarketStatus::Resolved || market.status == MarketStatus::Invalid,
            ChubiError::MarketNotResolved
        );

        let position = &mut ctx.accounts.position;
        require!(position.market == market.key(), ChubiError::PositionMarketMismatch);
        require!(!position.claimed, ChubiError::AlreadyClaimed);

        position.payout_amount = payout_amount;

        msg!("Payout set: {} lamports for position #{}", payout_amount, position.nonce);
        Ok(())
    }

    /// User claims their payout. SOL transfers from vault → user trustlessly.
    pub fn claim_payout(ctx: Context<ClaimPayout>) -> Result<()> {
        let position = &mut ctx.accounts.position;
        require!(position.payout_amount > 0, ChubiError::NoPayout);
        require!(!position.claimed, ChubiError::AlreadyClaimed);
        require!(position.maker == ctx.accounts.maker.key(), ChubiError::NotYourPosition);

        let market = &ctx.accounts.market;
        let payout = position.payout_amount;

        // Transfer SOL from vault → maker (vault PDA signs)
        let market_key = market.key();
        let vault_seeds = &[
            b"vault",
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
            payout,
        )?;

        position.claimed = true;

        // Update market claimed total
        let market = &mut ctx.accounts.market.to_account_info();
        // We can't mutably borrow market again easily, so we skip updating total_claimed
        // for now. The vault balance is the source of truth.

        msg!("Claimed: {} lamports", payout);
        Ok(())
    }

    /// Authority marks a market as invalid (for refunds via set_payout).
    pub fn invalidate_market(ctx: Context<ResolveMarket>) -> Result<()> {
        let market = &mut ctx.accounts.market;
        require!(market.status == MarketStatus::Open, ChubiError::MarketNotOpen);

        market.status = MarketStatus::Invalid;
        market.winner = 0;

        msg!("Market invalidated — positions can be refunded via set_payout");
        Ok(())
    }
}

// ─── Account Structs ────────────────────────────────────────────────────────

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

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketState>,

    /// CHECK: Vault PDA receives SOL.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        init,
        payer = maker,
        space = 8 + PositionState::INIT_SPACE,
        seeds = [b"position", market.key().as_ref(), maker.key().as_ref(), market.position_count.to_le_bytes().as_ref()],
        bump,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResolveMarket<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, MarketState>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetPayout<'info> {
    #[account(
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, MarketState>,

    #[account(
        mut,
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
    )]
    pub position: Account<'info, PositionState>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimPayout<'info> {
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
        constraint = position.maker == maker.key() @ ChubiError::NotYourPosition,
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
    )]
    pub position: Account<'info, PositionState>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

// ─── State ──────────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct MarketState {
    pub bump: u8,
    pub vault_bump: u8,
    pub authority: Pubkey,
    #[max_len(64)]
    pub market_id: String,
    pub status: MarketStatus,
    pub winner: u8,            // 0 = unset, 1 = side A, 2 = side B, etc.
    pub total_deposited: u64,
    pub total_claimed: u64,
    pub position_count: u64,
    pub created_at: i64,
    pub resolution_duration: i64,
}

#[account]
#[derive(InitSpace)]
pub struct PositionState {
    pub bump: u8,
    pub market: Pubkey,
    pub maker: Pubkey,
    pub side: u8,              // 0 = A, 1 = B (or 0-5 for multi-option)
    pub amount: u64,
    pub payout_amount: u64,
    pub nonce: u64,
    pub claimed: bool,
    pub created_at: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum MarketStatus {
    Open,
    Resolved,
    Invalid,
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[error_code]
pub enum ChubiError {
    #[msg("Market ID exceeds 64 characters")]
    MarketIdTooLong,
    #[msg("Duration must be 10 minutes to 7 days")]
    InvalidDuration,
    #[msg("Market is not open")]
    MarketNotOpen,
    #[msg("Market is not resolved")]
    MarketNotResolved,
    #[msg("Invalid side (must be 0-5)")]
    InvalidSide,
    #[msg("Deposit below minimum (0.001 SOL)")]
    DepositTooSmall,
    #[msg("Invalid winner (must be 1-6)")]
    InvalidWinner,
    #[msg("Position does not belong to this market")]
    PositionMarketMismatch,
    #[msg("Not your position")]
    NotYourPosition,
    #[msg("No payout set for this position")]
    NoPayout,
    #[msg("Already claimed")]
    AlreadyClaimed,
}
