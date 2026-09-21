use anchor_lang::prelude::*;
use crate::constants::MAX_SIDES;

// ─── Market ────────────────────────────────────────────────────────────────
//
// Deliberately smaller than chubi-escrow's MarketState: a parimutuel pool has
// no conviction to track. There is no weighted pool, no cumulative TWD, no
// lockout, no penalty pool — the only thing that decides a payout is how much
// SOL sits on each outcome when betting closes.

#[account]
#[derive(InitSpace)]
pub struct BetMarket {
    // ── Identity & auth ──
    pub bump: u8,
    pub vault_bump: u8,
    /// Resolver. Bet markets settle against a real-world outcome, so unlike
    /// conviction markets there is nothing the chain can derive on its own —
    /// this key is the oracle.
    pub authority: Pubkey,
    #[max_len(64)]
    pub market_id: String,

    // ── Lifecycle ──
    pub status: BetStatus,
    /// Winning outcome (1-indexed: 1-6). 0 = unset.
    pub winner: u8,
    /// Number of outcomes (2-6).
    pub num_sides: u8,
    pub created_at: i64,
    /// Seconds from `created_at` until betting closes. Resolution happens at
    /// or after that instant, whenever the authority reports the outcome.
    pub close_duration: i64,
    pub resolved_at: i64,

    // ── Aggregates ──
    pub total_staked: u64,
    pub total_paid: u64,
    pub bet_count: u64,
    /// Lamports staked per outcome. This IS the price: an outcome holding 62%
    /// of the pot is showing 62¢.
    pub pools: [u64; MAX_SIDES],
    pub side_bet_counts: [u32; MAX_SIDES],

    // ── Fees ──
    /// Protocol fees accrued in the vault, awaiting `collect_fees`.
    pub protocol_fee_collected: u64,
    pub fee_recipient: Pubkey,

    // ── Optional dispute layer (real-money tier) ──────────────────────────
    //
    // A market with `dispute_window == 0` behaves exactly as before: the
    // authority calls `resolve_market` and it settles immediately. When the
    // window is non-zero, the authority instead `propose_outcome`s: the market
    // enters `Proposed`, and settlement waits `dispute_window` seconds during
    // which anyone can `dispute` by posting `DISPUTE_BOND` lamports. No dispute
    // → anyone `finalize`s after the window (permissionless). A dispute → the
    // authority arbitrates via `resolve_disputed`, which also decides the bond:
    // upheld dispute returns it to the disputer, rejected dispute forfeits it to
    // the fee recipient. This is the on-chain, bonded counterpart to the
    // off-chain ResolverRouter window — a real economic cost to challenge, so a
    // frivolous dispute is not free.
    /// Seconds a proposed outcome must wait before it can be finalized. 0 keeps
    /// the pre-dispute behavior (immediate `resolve_market`).
    pub dispute_window: i64,
    /// Winner proposed but not yet final (1-indexed; 0 = none proposed).
    pub proposed_winner: u8,
    /// When the current outcome was proposed (0 = none).
    pub proposed_at: i64,
    /// Who raised a dispute (Pubkey::default() = none). Their bond sits in the
    /// vault until `resolve_disputed` returns or forfeits it.
    pub disputer: Pubkey,
    /// Bond posted with the active dispute, in lamports.
    pub dispute_bond: u64,
}

// ─── Creator (side-car PDA) ────────────────────────────────────────────────
//
// seeds = ["creator", market.key()]. Always created by `create_market`;
// `creator == Pubkey::default()` means the market pays no creator commission.

#[account]
#[derive(InitSpace)]
pub struct BetCreator {
    pub bump: u8,
    pub market: Pubkey,
    pub creator: Pubkey,
    /// Lamports earned but not yet swept.
    pub fee_collected: u64,
    /// Lifetime total swept (telemetry / display).
    pub fee_claimed: u64,
}

// ─── Bet ───────────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct Bet {
    pub bump: u8,
    pub market: Pubkey,
    pub bettor: Pubkey,
    /// Outcome index (0-5).
    pub side: u8,
    /// Stake in lamports.
    pub amount: u64,
    /// Filled in on claim, for the record.
    pub payout_amount: u64,
    /// Client-chosen PDA seed. Same reasoning as chubi-escrow: a market-global
    /// counter would make two bettors building transactions at the same moment
    /// derive the same address and the second one fail.
    pub nonce: u64,
    pub claimed: bool,
    pub created_at: i64,
    /// The price the bettor got, PRECISION-scaled: this outcome's share of the
    /// pot immediately after the stake landed. Display only — the settlement
    /// price is whatever the pools say at close. Stored because a parimutuel
    /// bettor otherwise has no record of the odds they accepted.
    pub entry_odds: u64,
}

// ─── Enums ─────────────────────────────────────────────────────────────────

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum BetStatus {
    Open,
    /// An outcome has been proposed and is inside its dispute window. Betting
    /// is closed; the market is not yet payable.
    Proposed,
    /// A proposed outcome was challenged with a bond; awaiting authority
    /// arbitration via `resolve_disputed`.
    Disputed,
    Resolved,
    Invalid,
}
