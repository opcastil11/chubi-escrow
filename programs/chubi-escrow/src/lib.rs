use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod math;
pub mod state;

use instructions::*;

declare_id!("DUigKbzHrJTdEmAstpisqV1cQT951kkHk4PdMB5i4BbJ");

#[program]
pub mod chubi_escrow {
    use super::*;

    /// Create a new conviction market with vault PDA + creator side-car.
    /// Pass `creator = Pubkey::default()` for anonymous markets that don't
    /// pay a creator commission (e.g. seed/system markets).
    pub fn create_market(
        ctx: Context<CreateMarket>,
        market_id: String,
        resolution_duration: i64,
        num_sides: u8,
        allow_withdrawal: bool,
        enable_lockout: bool,
        fee_recipient: Pubkey,
        creator: Pubkey,
    ) -> Result<()> {
        instructions::create_market::handler(
            ctx, market_id, resolution_duration, num_sides,
            allow_withdrawal, enable_lockout, fee_recipient, creator,
        )
    }

    /// Deposit SOL into a market side. Entry weight computed on-chain.
    /// `min_weight` is a slippage guard: revert if the computed entry weight
    /// (after any lockout / fraction_remaining drift) is below this value.
    /// Pass 0 to disable the guard.
    pub fn deposit(ctx: Context<Deposit>, side: u8, amount: u64, min_weight: u64) -> Result<()> {
        instructions::deposit::handler(ctx, side, amount, min_weight)
    }

    /// Permissionless resolution — anyone can call after market expires.
    /// Winner determined on-chain by TWD.
    pub fn resolve_market(ctx: Context<ResolveMarket>) -> Result<()> {
        instructions::resolve_market::handler(ctx)
    }

    /// Authority-only force resolution with specified winner.
    pub fn admin_resolve(ctx: Context<AdminResolve>, winner: u8) -> Result<()> {
        instructions::admin_resolve::handler(ctx, winner)
    }

    /// Claim payout — computed on-chain via TWCD formula.
    pub fn claim_payout(ctx: Context<ClaimPayout>) -> Result<()> {
        instructions::claim_payout::handler(ctx)
    }

    /// Withdraw position with time-based penalty (if market allows it).
    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        instructions::withdraw::handler(ctx)
    }

    /// Authority marks market as invalid (for full refunds).
    pub fn invalidate_market(ctx: Context<InvalidateMarket>) -> Result<()> {
        instructions::invalidate::handler(ctx)
    }

    /// Refund full deposit from an invalid market.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        instructions::refund::handler(ctx)
    }

    /// Close a settled position PDA, returning rent to maker.
    pub fn close_position(ctx: Context<ClosePosition>) -> Result<()> {
        instructions::close_position::handler(ctx)
    }

    /// Authority sweeps accumulated protocol fees from vault.
    pub fn collect_fees(ctx: Context<CollectFees>) -> Result<()> {
        instructions::collect_fees::handler(ctx)
    }

    /// Creator sweeps accumulated 0.5% commission from vault → creator wallet.
    /// Reverts if signer != creator stored on the side-car.
    pub fn claim_creator_fees(ctx: Context<ClaimCreatorFees>) -> Result<()> {
        instructions::claim_creator_fees::handler(ctx)
    }
}
