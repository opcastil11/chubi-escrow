/// Fixed-point precision: 6 decimal places.
pub const PRECISION: u64 = 1_000_000;

/// Basis-point scale (10_000 = 100%).
pub const BPS_SCALE: u64 = 10_000;

/// Protocol fee on profit at exit: 2% (matches chubi-escrow timed markets).
pub const PROTOCOL_FEE_BPS: u64 = 200;

/// Creator commission on profit at exit: 0.5% (matches chubi-escrow).
pub const CREATOR_FEE_BPS: u64 = 50;

/// Minimum entry weight (0.9x). Late depositors get this floor.
///
/// The original 0.3x floor combined with the multiplicative exit formula
/// (`amount × entry_weight / weighted_pools[side] × pools[side]`) meant a
/// late depositor could lose up to ~70% of their principal on the spot to
/// earlier holders on the same side — even with zero funding flow. Raising
/// the floor to 0.9 caps that worst-case dilution at ~10%, which is a
/// band-aid while a principal/funding split (see review §1, R3a) is
/// designed. The curve still favors early conviction — early entrants get
/// up to 1.0x and a larger share of funding — without bleeding principal.
pub const MIN_WEIGHT_FLOOR: u64 = 900_000;

/// Minimum deposit: 0.02 SOL = 20_000_000 lamports.
///
/// Sized to comfortably exceed the rent of a `PerpPositionState` account
/// (~0.00165 SOL for ~115 bytes). With `exit_perpetual` auto-closing the
/// position and refunding rent to the maker, the floor only needs to keep
/// the deposit economically meaningful relative to overhead — 0.02 SOL is
/// ~12× the position's rent, so even worst-case trip costs (deposit + exit
/// signature fees + LUT) stay a small fraction of principal.
pub const MIN_DEPOSIT_LAMPORTS: u64 = 20_000_000;

/// Market ID maximum length.
pub const MAX_MARKET_ID_LEN: usize = 64;

/// Binary perpetuals only in v1 — multi-option streaming-funding gets weird.
pub const NUM_SIDES: u8 = 2;

/// Reference window for entry-weight decay (seconds). 1 year. The curve is
/// `0.3 + 0.7 * (remaining/REF_WINDOW)^2`, so a depositor at month 6 gets
/// ~0.475x weight; month 12 hits the floor. Perpetuals don't expire — this is
/// purely about handicapping late entrants.
pub const REFERENCE_WINDOW_SECS: i64 = 365 * 86_400;

/// Streaming funding: how often `crank_epoch` can fire.
pub const EPOCH_SECS: i64 = 3_600;

/// Base funding per epoch, as fraction of the loser pool, scaled by current
/// pool imbalance. Effective rate ≈ BASE × imbalance, so a balanced market
/// pays nothing and a 100/0 split pays the full base. With 10 bps base, the
/// dominant side gains roughly 0.05–0.07%/h on typical 60/40 → 80/20 pools
/// (~1–1.5%/day, ~10%/week). Tunable via redeploy.
pub const FUNDING_RATE_BPS: u64 = 10;

/// Cranker rebate: fraction of each epoch's funding paid out to the caller of
/// `crank_epoch`. Drains from the vault, not the winner pool — so the loser
/// loses its full `funding` lamports, but the winner pool only receives
/// `funding × (1 - rebate)`. The delta goes straight to whoever cranked.
///
/// 5% of e.g. a 60k-lamport funding on a 100-SOL market = 3k lamports — still
/// below a 5k sig fee on tiny markets, but for any non-trivial pool the
/// permissionless crank becomes self-sustaining. Removes the single-point-of-
/// failure of "authority must keep cranking with its own SOL".
pub const CRANKER_REBATE_BPS: u64 = 500;

/// Wallets allowed to launch perpetual markets. Hardcoded — adding/removing
/// requires a redeploy. Position 0 is the protocol authority; position 1 is
/// the day-to-day admin wallet.
pub const PERPETUAL_ADMINS: [anchor_lang::prelude::Pubkey; 2] = [
    anchor_lang::pubkey!("4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe"),
    anchor_lang::pubkey!("DJ2oA3sVMcrSPxQmvzJczbJWcXKBo8v9DraShaNVLko6"),
];
