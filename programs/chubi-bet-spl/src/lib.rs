//! # chubi-bet — plain parimutuel betting
//!
//! The "Bet" mode: pick an outcome, stake SOL, winners split the pot in
//! proportion to their stake. That is the entire mechanism.
//!
//! It is a separate program from `chubi-escrow` on purpose. Conviction markets
//! settle on TWCD — time-weighted dominance, entry-weight decay, swing
//! penalties, lockout windows — and every one of those is state a bet market
//! must not have. Bolting a mode flag onto `MarketState` would have carried
//! ~200 bytes of dead conviction accounting into every bet, and left the
//! payout path branching on a boolean where a mistake pays the wrong people.
//! Two programs, two settlement rules, no shared failure mode.
//!
//! Differences from chubi-escrow, by design:
//!   * no entry weight — a bet placed one second before close pays the same
//!     multiple as the first bet of the market
//!   * no TWCD, no swing penalty — losers' stakes go entirely to winners
//!   * no lockout, no withdrawal — there is no Sell; a stake is committed
//!   * resolution is authority-only, because the outcome is a real-world fact
//!     the chain cannot derive
//!
//! What it keeps: the vault PDA layout, the client-chosen nonce for position
//! addresses, the 2% protocol + 0.5% creator fees charged on profit only, and
//! the invalidate/refund escape hatch.

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod math;
pub mod state;

use instructions::*;

declare_id!("4ZhafuH1tQwqD3u6Kxjt9U2eE3ewHGxuuAsjBySJbyya");

#[program]
pub mod chubi_bet_spl {
    use super::*;

    /// Create a bet market with its vault and creator side-car.
    /// `creator = Pubkey::default()` for house markets that pay no commission.
    /// `dispute_window` (seconds, 0 = none) enables the propose/dispute/finalize
    /// flow; with 0 the market uses `resolve_market` and settles immediately.
    pub fn create_market(
        ctx: Context<CreateBetMarket>,
        market_id: String,
        close_duration: i64,
        num_sides: u8,
        fee_recipient: Pubkey,
        creator: Pubkey,
        dispute_window: i64,
    ) -> Result<()> {
        instructions::create_market::handler(
            ctx, market_id, close_duration, num_sides, fee_recipient, creator, dispute_window,
        )
    }

    /// Stake SOL on an outcome.
    ///
    /// `max_odds` is a slippage guard, PRECISION-scaled: revert if this
    /// outcome's pool share AFTER the stake lands exceeds it (a higher share
    /// means a lower payout multiple). Pass 0 to disable. It bounds drift
    /// between click and landing only — later bets keep moving the pool, which
    /// is inherent to parimutuel.
    ///
    /// `nonce` is chosen by the CLIENT and is the last seed of the bet PDA, so
    /// concurrent bettors never collide on the same address.
    pub fn place_bet(
        ctx: Context<PlaceBet>,
        side: u8,
        amount: u64,
        max_odds: u64,
        nonce: u64,
    ) -> Result<()> {
        instructions::place_bet::handler(ctx, side, amount, max_odds, nonce)
    }

    /// Authority (oracle) reports the winning outcome. 1-indexed.
    pub fn resolve_market(ctx: Context<ResolveBetMarket>, winner: u8) -> Result<()> {
        instructions::resolve_market::handler(ctx, winner)
    }

    // ── Dispute layer (markets created with dispute_window > 0) ──────────────

    /// Authority proposes an outcome. The market enters `Proposed` and waits out
    /// its dispute window before it can be finalized or arbitrated.
    pub fn propose_outcome(ctx: Context<ProposeOutcome>, winner: u8) -> Result<()> {
        instructions::dispute::propose_handler(ctx, winner)
    }

    /// Anyone challenges a proposed outcome inside the window by posting a bond.
    pub fn dispute(ctx: Context<DisputeOutcome>) -> Result<()> {
        instructions::dispute::dispute_handler(ctx)
    }

    /// Permissionless: settle an undisputed proposed outcome after the window.
    pub fn finalize(ctx: Context<FinalizeOutcome>) -> Result<()> {
        instructions::dispute::finalize_handler(ctx)
    }

    /// Authority arbitrates a disputed market and settles the bond in the same
    /// call (returned to the disputer if upheld, forfeited to fees if rejected).
    pub fn resolve_disputed(ctx: Context<ResolveDisputed>, winner: u8) -> Result<()> {
        instructions::dispute::resolve_disputed_handler(ctx, winner)
    }

    /// Winner claims `stake / winner_pool * total_pool`, minus fees on profit.
    pub fn claim_winnings(ctx: Context<ClaimWinnings>) -> Result<()> {
        instructions::claim_winnings::handler(ctx)
    }

    /// Authority voids the market — every bet becomes refundable at full stake.
    pub fn invalidate_market(ctx: Context<InvalidateBetMarket>) -> Result<()> {
        instructions::invalidate::handler(ctx)
    }

    /// Full stake back from a voided market.
    pub fn refund(ctx: Context<RefundBet>) -> Result<()> {
        instructions::refund::handler(ctx)
    }

    /// Close a settled bet PDA, returning rent. Losing bets count as settled —
    /// they can never be claimed, so this is their only exit.
    pub fn close_bet(ctx: Context<CloseBet>) -> Result<()> {
        instructions::close_bet::handler(ctx)
    }

    /// Authority sweeps accrued protocol fees from the vault.
    pub fn collect_fees(ctx: Context<CollectBetFees>) -> Result<()> {
        instructions::collect_fees::handler(ctx)
    }

    /// Creator sweeps accrued commission from the vault.
    pub fn claim_creator_fees(ctx: Context<ClaimBetCreatorFees>) -> Result<()> {
        instructions::claim_creator_fees::handler(ctx)
    }
}
