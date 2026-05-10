use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::ChubiError;
use crate::events;
use crate::math;
use crate::state::*;

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
        constraint = position.market == market.key() @ ChubiError::PositionMarketMismatch,
        constraint = position.maker == maker.key() @ ChubiError::NotYourPosition,
    )]
    pub position: Account<'info, PositionState>,

    /// Optional CreatorAccount side-car. Markets created with the new
    /// `create_market` always have one (creator = Pubkey::default() for
    /// anonymous). Pre-CreatorAccount legacy markets do NOT, so the caller
    /// passes the canonical PDA address as an UncheckedAccount and the
    /// handler decides whether to charge the creator fee based on whether
    /// the account is initialized.
    #[account(
        mut,
        seeds = [b"creator", market.key().as_ref()],
        bump,
    )]
    /// CHECK: Validated in handler — may be uninitialized for legacy markets.
    pub creator_account: UncheckedAccount<'info>,

    #[account(mut)]
    pub maker: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimPayout>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let position = &mut ctx.accounts.position;

    require!(market.status == MarketStatus::Resolved, ChubiError::MarketNotResolved);
    require!(!position.claimed, ChubiError::AlreadyClaimed);
    require!(!position.withdrawn, ChubiError::AlreadyWithdrawn);

    // Winner is 1-indexed, position.side is 0-indexed
    let winner_idx = market.winner.checked_sub(1).ok_or(error!(ChubiError::InvalidWinner))?;
    let is_winner = position.side == winner_idx;

    // Compute total distributable: active pools + penalty pool
    let total_distributable = math::sum_pools(&market.pools, market.num_sides)
        .checked_add(market.penalty_pool).ok_or(error!(ChubiError::MathOverflow))?;

    // Resolve creator fee bps: 0 if the side-car is uninitialized OR if it
    // exists with creator = Pubkey::default() (anonymous market, no fees).
    let creator_account_info = &ctx.accounts.creator_account;
    let has_creator = is_creator_account_active(creator_account_info, market.key())?;
    let creator_fee_bps = if has_creator { CREATOR_FEE_BPS } else { 0 };

    let (payout, fee, creator_fee) = if is_winner {
        math::compute_winner_payout(
            position.amount,
            position.entry_weight,
            winner_idx,
            &market.pools,
            &market.weighted_pools,
            market.winner_payout_share,
            total_distributable,
            creator_fee_bps,
        )?
    } else {
        let loser_payout = math::compute_loser_payout(
            position.amount,
            position.entry_weight,
            position.side,
            &market.weighted_pools,
            market.winner_payout_share,
            total_distributable,
        )?;
        (loser_payout, 0u64, 0u64)
    };

    require!(payout > 0, ChubiError::NoPayout);

    // Cap payout so the vault keeps its rent-exempt minimum.
    // The rent will be recovered later via collect_fees or
    // when the authority eventually closes the vault.
    let vault_balance = ctx.accounts.vault.lamports();
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(0); // SystemAccount = 0 data bytes
    let available = vault_balance.saturating_sub(min_rent);
    let actual_payout = payout.min(available);
    require!(actual_payout > 0, ChubiError::InsufficientVault);

    // Transfer SOL from vault → maker (vault PDA signs)
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
                to: ctx.accounts.maker.to_account_info(),
            },
            &[vault_seeds],
        ),
        actual_payout,
    )?;

    // Update state
    position.claimed = true;
    position.payout_amount = actual_payout;
    market.total_claimed = market.total_claimed
        .checked_add(actual_payout).ok_or(error!(ChubiError::MathOverflow))?;
    market.protocol_fee_collected = market.protocol_fee_collected
        .checked_add(fee).ok_or(error!(ChubiError::MathOverflow))?;

    // Accumulate creator fee on the side-car (if active).
    if has_creator && creator_fee > 0 {
        let mut data = creator_account_info.try_borrow_mut_data()?;
        let mut creator_account = CreatorAccount::try_deserialize(&mut &data[..])?;
        creator_account.fee_collected = creator_account.fee_collected
            .checked_add(creator_fee).ok_or(error!(ChubiError::MathOverflow))?;
        let mut writer = &mut data[..];
        creator_account.try_serialize(&mut writer)?;
    }

    emit!(events::PayoutClaimed {
        market_id: market.market_id.clone(),
        maker: position.maker,
        nonce: position.nonce,
        payout: actual_payout,
        fee,
        creator_fee,
        is_winner,
    });

    Ok(())
}

/// True if `account` is an initialized CreatorAccount whose `market` field
/// matches the expected market AND `creator` is not Pubkey::default()
/// (anonymous markets carry a side-car but pay no fees).
fn is_creator_account_active(
    account: &UncheckedAccount<'_>,
    expected_market: Pubkey,
) -> Result<bool> {
    // Uninitialized account → System-owned with zero data. Legacy markets
    // pre-dating this feature land here.
    if account.data_is_empty() || account.owner != &crate::ID {
        return Ok(false);
    }
    let data = account.try_borrow_data()?;
    let creator_account = CreatorAccount::try_deserialize(&mut &data[..])?;
    require!(
        creator_account.market == expected_market,
        ChubiError::CreatorMarketMismatch
    );
    Ok(creator_account.creator != Pubkey::default())
}
