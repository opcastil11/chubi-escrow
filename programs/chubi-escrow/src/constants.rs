/// Fixed-point precision: 6 decimal places.
pub const PRECISION: u64 = 1_000_000;

/// Protocol fee: 2% on winner profits only.
pub const PROTOCOL_FEE_BPS: u64 = 200;

/// Basis-point scale (10_000 = 100%).
pub const BPS_SCALE: u64 = 10_000;

/// Minimum entry weight (0.3x). Late depositors get this floor.
pub const MIN_WEIGHT_FLOOR: u64 = 300_000;

/// Dominance multiplier: 1.5x expressed as numerator/denominator.
pub const DOMINANCE_NUM: u64 = 3;
pub const DOMINANCE_DENOM: u64 = 2;

/// Dynamic lockout bounds (fraction of PRECISION).
pub const MIN_LOCKOUT: u64 = 50_000; // 5%
pub const MAX_LOCKOUT: u64 = 250_000; // 25%

/// Minimum positions for "healthy" market depth.
pub const MIN_HEALTHY_POSITIONS: u64 = 10;

/// Minimum deposit: 0.001 SOL = 1_000_000 lamports.
pub const MIN_DEPOSIT_LAMPORTS: u64 = 1_000_000;

/// Safety floor: winner_payout_share must exceed winner_fraction by at least 1%.
pub const MIN_TRANSFER_BPS: u64 = 10_000; // 1% of PRECISION

/// Market ID maximum length.
pub const MAX_MARKET_ID_LEN: usize = 64;

/// Maximum sides per market.
pub const MAX_SIDES: usize = 6;

/// Duration bounds (seconds).
pub const MIN_DURATION_SECS: i64 = 600; // 10 minutes
pub const MAX_DURATION_SECS: i64 = 604_800; // 7 days

/// Duration threshold for entry weight exponent selection (seconds).
/// Markets shorter than this use linear decay; longer markets use quadratic.
/// (Cubic decay was removed: it produced near-floor weight by the market midpoint
///  on multi-day markets, killing late-entry economics.)
pub const SHORT_DURATION_SECS: i64 = 1_800; // 30 min — exponent 1
// >= 30 min: exponent 2 (quadratic)
