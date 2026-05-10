use anchor_lang::prelude::*;

// ─── Perpetual Market ──────────────────────────────────────────────────────
//
// One per perpetual. Streaming-funding-rate variant of the timed conviction
// market. Pool sizes shift over time as `crank_epoch` transfers lamports from
// the smaller to the larger pool, scaled by the current pool imbalance.

#[account]
#[derive(InitSpace)]
pub struct PerpMarketState {
    pub bump: u8,
    pub vault_bump: u8,
    pub authority: Pubkey,
    #[max_len(64)]
    pub market_id: String,

    /// Always 2 in v1 (binary perpetuals only). Kept for forward-compat.
    pub num_sides: u8,
    /// True after admin force-closes the market (no more deposits / funding /
    /// exits beyond settling existing positions). Reuses the existing close
    /// flow instead of mirroring the timed-market lifecycle.
    pub is_closed: bool,

    pub created_at: i64,
    /// Last time `crank_epoch` ran. Funding can fire again at last_epoch_at + EPOCH_SECS.
    pub last_epoch_at: i64,

    // ── Pool tracking ──
    /// Raw lamports allocated to each side. Funding shifts these between sides.
    pub pools: [u64; 2],
    /// Sum of (amount * entry_weight) per side. Set on deposit, decremented on
    /// exit. The withdrawer's share of the pool is `my_weighted / weighted_pools[side]`.
    pub weighted_pools: [u128; 2],
    /// Active position count per side (used for analytics / depth checks).
    pub side_position_counts: [u32; 2],
    /// Timestamp of first deposit on each side.
    pub first_deposit_at: [i64; 2],

    // ── Aggregates ──
    pub total_deposited: u64,
    pub total_exited: u64,
    pub position_count: u64,
    /// Lifetime sum of funding lamports moved by `crank_epoch`. Telemetry only.
    pub cumulative_funding_paid: u64,

    // ── Fee tracking ──
    pub protocol_fee_collected: u64,
    pub fee_recipient: Pubkey,
}

// ─── Position ──────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct PerpPositionState {
    pub bump: u8,
    pub market: Pubkey,
    pub maker: Pubkey,
    /// 0 or 1 (binary).
    pub side: u8,
    /// Original deposit (lamports). Used as the principal for profit calc.
    pub amount: u64,
    /// Position index within the market (PDA seed).
    pub nonce: u64,
    pub created_at: i64,
    /// Entry weight at deposit time (PRECISION-scaled, 300_000 to 1_000_000).
    /// Determines fractional ownership of the side pool.
    pub entry_weight: u64,
    pub exited: bool,
    /// Net lamports paid out on exit (post-fees). Set when `exit_perpetual` runs.
    pub exit_payout: u64,
}

// ─── Creator (side-car PDA) ────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct CreatorAccount {
    pub bump: u8,
    pub market: Pubkey,
    pub creator: Pubkey,
    pub fee_collected: u64,
    pub fee_claimed: u64,
}
