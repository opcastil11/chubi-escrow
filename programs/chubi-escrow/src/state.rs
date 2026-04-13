use anchor_lang::prelude::*;
use crate::constants::MAX_SIDES;

// ─── Market ────────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct MarketState {
    // ── Identity & auth ──
    pub bump: u8,
    pub vault_bump: u8,
    pub authority: Pubkey,
    #[max_len(64)]
    pub market_id: String,

    // ── Lifecycle ──
    pub status: MarketStatus,
    /// Winning side (1-indexed: 1-6). 0 = unset.
    pub winner: u8,
    /// Number of sides (2-6).
    pub num_sides: u8,
    pub created_at: i64,
    pub resolution_duration: i64,
    pub resolved_at: i64,

    // ── Aggregate counters ──
    pub total_deposited: u64,
    pub total_claimed: u64,
    pub position_count: u64,

    // ── Per-side pool tracking ──
    /// Raw active deposits per side.
    pub pools: [u64; MAX_SIDES],
    /// Sum of (amount * entry_weight) per side. u128 to prevent overflow.
    pub weighted_pools: [u128; MAX_SIDES],
    /// Active position count per side.
    pub side_position_counts: [u32; MAX_SIDES],
    /// Timestamp of first deposit on each side (0 = no deposit yet).
    pub first_deposit_at: [i64; MAX_SIDES],

    // ── Withdrawal penalty pool ──
    /// Accumulated penalties from withdrawals (stays in vault, benefits winners).
    pub penalty_pool: u64,

    // ── Incremental TWD tracking ──
    /// Cumulative (share_0 * elapsed_seconds). Only updated when has_both_sides.
    pub cumulative_twd_0: u128,
    /// Total seconds tracked for TWD.
    pub cumulative_time: u64,
    /// Last timestamp TWD was updated.
    pub last_snapshot_at: i64,
    /// Cached: at least 2 sides have active deposits.
    pub has_both_sides: bool,

    // ── Resolution data (set at resolve time) ──
    /// Fraction of total pool allocated to winners (PRECISION-scaled).
    pub winner_payout_share: u64,
    /// Final time-weighted dominance for side 0 (PRECISION-scaled).
    pub twd_0: u64,

    // ── Configuration ──
    pub allow_withdrawal: bool,
    pub enable_lockout: bool,

    // ── Fee tracking ──
    /// Accumulated protocol fees sitting in vault (not yet swept).
    pub protocol_fee_collected: u64,
    /// Where fees go when collected.
    pub fee_recipient: Pubkey,
}

// ─── Position ──────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct PositionState {
    pub bump: u8,
    pub market: Pubkey,
    pub maker: Pubkey,
    /// Side index (0-5).
    pub side: u8,
    /// Deposit amount in lamports.
    pub amount: u64,
    /// Computed payout (set after claim, for records).
    pub payout_amount: u64,
    /// Position index within the market (used in PDA seed).
    pub nonce: u64,
    pub claimed: bool,
    pub created_at: i64,
    /// Early-bird weight (PRECISION-scaled, 300_000 to 1_000_000).
    pub entry_weight: u64,
    /// True if withdrawn before resolution.
    pub withdrawn: bool,
}

// ─── Enums ─────────────────────────────────────────────────────────────────

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum MarketStatus {
    Open,
    Resolved,
    Invalid,
}
