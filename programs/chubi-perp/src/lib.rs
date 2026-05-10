use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod math;
pub mod state;

use instructions::*;

declare_id!("JBo8FAveHuB55ZXjSeBAQL8ekaEae1btnftdNaQsjz3u");

/// Chubi perpetual conviction markets.
///
/// A streaming-funding-rate variant of the timed conviction market in
/// `chubi-escrow`. There is no expiry timer and no terminal resolution event.
/// Instead, an off-chain (or any) keeper calls `crank_epoch` every hour; that
/// instruction transfers a small amount of lamports from the smaller (losing)
/// pool to the larger (winning) pool, scaled by current pool imbalance. Each
/// holder's exit value is `(amount × entry_weight / weighted_pools[side]) ×
/// pools[side]`, so as funding flows, your position gains or loses value in
/// real time.
///
/// Holders exit any time via `exit_perpetual`. No fixed lockup, no withdrawal
/// penalty — fees apply to profit only (2% protocol + 0.5% creator).
///
/// Only wallets in `PERPETUAL_ADMINS` can launch new perpetuals.
#[program]
pub mod chubi_perp {
    use super::*;

    /// Authority-signed (admin-gated). `creator` is the wallet attribution and
    /// must be in PERPETUAL_ADMINS.
    pub fn create_perp_market(
        ctx: Context<CreatePerpMarket>,
        market_id: String,
        fee_recipient: Pubkey,
        creator: Pubkey,
    ) -> Result<()> {
        instructions::create_perp_market::handler(ctx, market_id, fee_recipient, creator)
    }

    /// User-signed. Deposit lamports into one side. Entry weight derived from
    /// the market's age — late entrants get a smaller share of future funding.
    pub fn deposit(ctx: Context<PerpDeposit>, side: u8, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, side, amount)
    }

    /// Permissionless. Anyone can call after `EPOCH_SECS` since the last crank.
    /// Moves funding from the smaller pool to the larger pool.
    pub fn crank_epoch(ctx: Context<CrankEpoch>) -> Result<()> {
        instructions::crank_epoch::handler(ctx)
    }

    /// User-signed. Exit a position at current fair value. Profit-only fees apply.
    pub fn exit_perpetual(ctx: Context<ExitPerpetual>) -> Result<()> {
        instructions::exit_perpetual::handler(ctx)
    }

    /// Authority-only. Stops new deposits and crank_epochs; holders can still exit.
    pub fn close_perp_market(ctx: Context<ClosePerpMarket>) -> Result<()> {
        instructions::close_market::handler(ctx)
    }

    /// Creator-signed. Sweep accumulated 0.5% commission from vault → creator wallet.
    pub fn claim_creator_fees(ctx: Context<ClaimPerpCreatorFees>) -> Result<()> {
        instructions::claim_creator_fees::handler(ctx)
    }

    /// Authority-signed. Sweep accumulated 2% protocol fees from vault → fee recipient.
    pub fn collect_fees(ctx: Context<CollectPerpFees>) -> Result<()> {
        instructions::collect_fees::handler(ctx)
    }
}
