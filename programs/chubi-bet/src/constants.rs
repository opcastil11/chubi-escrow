/// Fixed-point precision: 6 decimal places. Shares/odds are expressed as
/// fractions of this (500_000 = 50% = 50¢ in the Polymarket-style UI).
pub const PRECISION: u64 = 1_000_000;

/// Protocol fee: 2% — charged on PROFIT only, never on the stake.
/// Same rate as chubi-escrow so the two modes feel identical to a bettor.
pub const PROTOCOL_FEE_BPS: u64 = 200;

/// Creator commission: 0.5% on profit only. Zero when the market's creator
/// side-car holds `Pubkey::default()` (anonymous / house market).
pub const CREATOR_FEE_BPS: u64 = 50;

/// Basis-point scale (10_000 = 100%).
pub const BPS_SCALE: u64 = 10_000;

/// Minimum bet: 0.001 SOL.
pub const MIN_BET_LAMPORTS: u64 = 1_000_000;

/// Market ID maximum length.
pub const MAX_MARKET_ID_LEN: usize = 64;

/// Maximum outcomes per market.
pub const MAX_SIDES: usize = 6;

/// Betting-window bounds (seconds): 10 minutes to 7 days.
pub const MIN_DURATION_SECS: i64 = 600;
pub const MAX_DURATION_SECS: i64 = 604_800;
