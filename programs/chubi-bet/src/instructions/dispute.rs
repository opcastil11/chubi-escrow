use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::constants::*;
use crate::errors::BetError;
use crate::events;
use crate::math;
use crate::state::*;

// ─── Shared validation ──────────────────────────────────────────────────────
//
// A winner is only settleable if at least two outcomes were backed and the
// winning outcome itself has stakes on it — same rule `resolve_market` enforces
// for the no-dispute path. Kept here so propose/arbitrate can't settle a market
// that `resolve_market` would have refused.
fn require_settleable(market: &BetMarket, winner: u8) -> Result<()> {
    require!(
        winner >= 1 && (winner as usize) <= market.num_sides as usize,
        BetError::InvalidWinner
    );
    let mut sides_with = 0u8;
    for i in 0..market.num_sides as usize {
        if market.pools[i] > 0 {
            sides_with += 1;
        }
    }
    require!(sides_with >= 2, BetError::InsufficientSides);
    require!(market.pools[(winner - 1) as usize] > 0, BetError::NoWinningBets);
    Ok(())
}

// ─── propose_outcome ─────────────────────────────────────────────────────────
//
// The authority reports an outcome on a market that has a dispute window. The
// market enters `Proposed` and cannot pay out until the window elapses (via
// `finalize`) or a dispute is arbitrated. For a market with `dispute_window == 0`
// this path is rejected — that market uses `resolve_market` and settles at once.

#[derive(Accounts)]
pub struct ProposeOutcome<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, BetMarket>,
    pub authority: Signer<'info>,
}

pub fn propose_handler(ctx: Context<ProposeOutcome>, winner: u8) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let now = Clock::get()?.unix_timestamp;

    require!(market.status == BetStatus::Open, BetError::MarketNotOpen);
    require!(market.dispute_window > 0, BetError::InvalidDisputeWindow);
    require_settleable(market, winner)?;

    market.status = BetStatus::Proposed;
    market.proposed_winner = winner;
    market.proposed_at = now;

    emit!(events::BetOutcomeProposed {
        market_id: market.market_id.clone(),
        winner,
        proposed_at: now,
        dispute_window: market.dispute_window,
        proposer: ctx.accounts.authority.key(),
    });
    Ok(())
}

// ─── dispute ──────────────────────────────────────────────────────────────────
//
// Anyone can challenge a proposed outcome inside the window by posting
// DISPUTE_BOND lamports into the vault. Only one active dispute at a time — the
// first challenger books it. The bond is returned on `resolve_disputed` if the
// dispute is upheld, forfeited to the fee recipient if rejected. Moving the
// market to `Disputed` stops `finalize` from settling the proposed outcome.

#[derive(Accounts)]
pub struct DisputeOutcome<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: vault PDA holding stakes; the bond is transferred in here.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(mut)]
    pub disputer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn dispute_handler(ctx: Context<DisputeOutcome>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;

    require!(market.status == BetStatus::Proposed, BetError::NotProposed);
    // Inside the window only. Once it closes, the outcome is finalizable and no
    // longer challengeable.
    require!(
        now < market.proposed_at + market.dispute_window,
        BetError::DisputeWindowClosed
    );

    // Pull the bond into the vault. It rides alongside the pot until arbitration
    // decides where it goes; the pot accounting never touches it because payouts
    // are computed from `pools`, not the raw vault balance.
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.disputer.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        DISPUTE_BOND,
    )?;

    market.status = BetStatus::Disputed;
    market.disputer = ctx.accounts.disputer.key();
    market.dispute_bond = DISPUTE_BOND;

    emit!(events::BetDisputed {
        market_id: market.market_id.clone(),
        disputer: ctx.accounts.disputer.key(),
        bond: DISPUTE_BOND,
        disputed_winner: market.proposed_winner,
    });
    Ok(())
}

// ─── finalize ───────────────────────────────────────────────────────────────
//
// Permissionless. After the dispute window closes on an undisputed `Proposed`
// market, anyone can finalize it — the proposed outcome becomes the winner and
// the market is payable. Mirrors chubi-escrow's permissionless resolve: the
// authority proposes, but the clock (not the authority) settles it, so a market
// can't be left proposed-but-unpaid forever.

#[derive(Accounts)]
pub struct FinalizeOutcome<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
    )]
    pub market: Account<'info, BetMarket>,
    pub finalizer: Signer<'info>,
}

pub fn finalize_handler(ctx: Context<FinalizeOutcome>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;

    require!(market.status == BetStatus::Proposed, BetError::NotProposed);
    require!(
        now >= market.proposed_at + market.dispute_window,
        BetError::DisputeWindowOpen
    );

    let winner = market.proposed_winner;
    // Re-check settleability: pools cannot change after `Proposed` (betting is
    // closed), but this keeps the invariant explicit and future-proof.
    require_settleable(market, winner)?;

    market.status = BetStatus::Resolved;
    market.winner = winner;
    market.resolved_at = now;

    emit!(events::BetMarketResolved {
        market_id: market.market_id.clone(),
        winner,
        winner_pool: market.pools[(winner - 1) as usize],
        total_pool: math::sum_pools(&market.pools, market.num_sides),
        resolver: ctx.accounts.finalizer.key(),
    });
    Ok(())
}

// ─── resolve_disputed ─────────────────────────────────────────────────────────
//
// Authority arbitrates a disputed market: it reports the true winner and, in
// the same instruction, the bond is settled. If the final winner differs from
// the proposed one the dispute was correct → bond returns to the disputer; if it
// matches, the challenge was wrong → bond is forfeited to the fee recipient. The
// disputer and fee_recipient accounts are passed explicitly and checked against
// the stored keys so the lamports can only move to the right party.

#[derive(Accounts)]
pub struct ResolveDisputed<'info> {
    #[account(
        mut,
        seeds = [b"market", market.market_id.as_bytes()],
        bump = market.bump,
        has_one = authority,
    )]
    pub market: Account<'info, BetMarket>,

    /// CHECK: vault PDA; the bond is paid out from here.
    #[account(
        mut,
        seeds = [b"vault", market.key().as_ref()],
        bump = market.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    /// CHECK: the wallet that raised the dispute; validated against
    /// `market.disputer`. Receives the bond back if the dispute is upheld.
    #[account(mut, address = market.disputer @ BetError::DisputerMismatch)]
    pub disputer: SystemAccount<'info>,

    /// CHECK: the market's fee recipient; validated against
    /// `market.fee_recipient`. Receives the bond if the dispute is rejected.
    #[account(mut, address = market.fee_recipient @ BetError::FeeRecipientMismatch)]
    pub fee_recipient: SystemAccount<'info>,

    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn resolve_disputed_handler(ctx: Context<ResolveDisputed>, winner: u8) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let bond;
    let upheld;
    let bond_to;
    {
        let market = &ctx.accounts.market;
        require!(market.status == BetStatus::Disputed, BetError::NotDisputed);
        require_settleable(market, winner)?;
        bond = market.dispute_bond;
        // Upheld = the arbitrated winner overturns what was proposed.
        upheld = winner != market.proposed_winner;
        bond_to = if upheld {
            ctx.accounts.disputer.key()
        } else {
            ctx.accounts.fee_recipient.key()
        };
    }

    // Pay the bond out of the vault. The vault is a SystemAccount (owned by the
    // System Program), so lamports move via a signed CPI transfer with the
    // vault PDA seeds — NOT a direct lamport borrow, which only works for
    // accounts this program owns. Same pattern as claim_winnings.
    if bond > 0 {
        let market_key = ctx.accounts.market.key();
        let vault_bump = ctx.accounts.market.vault_bump;
        let vault_seeds: &[&[u8]] = &[b"vault", market_key.as_ref(), &[vault_bump]];
        let dest = if upheld {
            ctx.accounts.disputer.to_account_info()
        } else {
            ctx.accounts.fee_recipient.to_account_info()
        };
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: dest,
                },
                &[vault_seeds],
            ),
            bond,
        )?;
    }

    let market = &mut ctx.accounts.market;
    market.status = BetStatus::Resolved;
    market.winner = winner;
    market.resolved_at = now;
    market.dispute_bond = 0;

    emit!(events::BetDisputeResolved {
        market_id: market.market_id.clone(),
        winner,
        upheld,
        bond_returned_to: bond_to,
        bond_amount: bond,
    });
    emit!(events::BetMarketResolved {
        market_id: market.market_id.clone(),
        winner,
        winner_pool: market.pools[(winner - 1) as usize],
        total_pool: math::sum_pools(&market.pools, market.num_sides),
        resolver: ctx.accounts.authority.key(),
    });
    Ok(())
}
