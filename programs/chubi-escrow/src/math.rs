use crate::constants::*;
use crate::state::MarketState;
use anchor_lang::prelude::*;

// ─── Entry Weight ──────────────────────────────────────────────────────────

/// Compute early-bird entry weight for a deposit.
///
/// Formula: weight = FLOOR + (PRECISION - FLOOR) * fraction^exp / PRECISION^(exp-1)
///
/// Exponent selected by market duration:
///   < 30 min  → 1 (linear)    — speed matches; full timeline is short anyway
///   >= 30 min → 2 (quadratic) — every other market
///
/// Returns weight in [MIN_WEIGHT_FLOOR, PRECISION].
pub fn compute_entry_weight(
    fraction_remaining: u64, // [0, PRECISION]
    resolution_duration: i64,
) -> Result<u64> {
    if fraction_remaining == 0 {
        return Ok(MIN_WEIGHT_FLOOR);
    }
    if fraction_remaining >= PRECISION {
        return Ok(PRECISION);
    }

    let frac = fraction_remaining as u128;
    let p = PRECISION as u128;
    let range = (PRECISION - MIN_WEIGHT_FLOOR) as u128; // 700_000

    let frac_pow = if resolution_duration < SHORT_DURATION_SECS {
        // Exponent 1: linear
        frac
    } else {
        // Exponent 2: quadratic — frac^2 / PRECISION
        frac.checked_mul(frac).ok_or(error!(ChubiError::MathOverflow))?
            .checked_div(p).ok_or(error!(ChubiError::MathOverflow))?
    };

    // weight = FLOOR + range * frac_pow / PRECISION
    let bonus = range
        .checked_mul(frac_pow).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(p).ok_or(error!(ChubiError::MathOverflow))?;

    let weight = MIN_WEIGHT_FLOOR as u128 + bonus;
    Ok(weight.min(PRECISION as u128) as u64)
}

use crate::errors::ChubiError;

// ─── TWD Update ────────────────────────────────────────────────────────────

/// Update the incremental time-weighted dominance tracker.
/// Must be called before any pool change (deposit, withdrawal) and at resolution.
pub fn update_twd(market: &mut MarketState, now: i64) -> Result<()> {
    let elapsed = now.saturating_sub(market.last_snapshot_at);
    if elapsed > 0 && market.has_both_sides {
        let total_pool = sum_pools(&market.pools, market.num_sides);
        if total_pool > 0 {
            // share_0 = pools[0] * PRECISION / total_pool
            let share_0 = (market.pools[0] as u128)
                .checked_mul(PRECISION as u128).ok_or(error!(ChubiError::MathOverflow))?
                .checked_div(total_pool as u128).ok_or(error!(ChubiError::MathOverflow))?;

            // cumulative_twd_0 += share_0 * elapsed
            let increment = share_0
                .checked_mul(elapsed as u128).ok_or(error!(ChubiError::MathOverflow))?;
            market.cumulative_twd_0 = market.cumulative_twd_0
                .checked_add(increment).ok_or(error!(ChubiError::MathOverflow))?;
            market.cumulative_time = market.cumulative_time
                .checked_add(elapsed as u64).ok_or(error!(ChubiError::MathOverflow))?;
        }
    }
    market.last_snapshot_at = now;

    // Recompute has_both_sides
    let mut sides_with_deposits = 0u8;
    for i in 0..market.num_sides as usize {
        if market.pools[i] > 0 {
            sides_with_deposits += 1;
        }
    }
    market.has_both_sides = sides_with_deposits >= 2;

    Ok(())
}

/// Sum active pools for the first `n` sides.
pub fn sum_pools(pools: &[u64; MAX_SIDES], num_sides: u8) -> u64 {
    let mut total: u64 = 0;
    for i in 0..num_sides as usize {
        total = total.saturating_add(pools[i]);
    }
    total
}

/// Sum active position counts.
pub fn total_positions(counts: &[u32; MAX_SIDES], num_sides: u8) -> u32 {
    let mut total: u32 = 0;
    for i in 0..num_sides as usize {
        total = total.saturating_add(counts[i]);
    }
    total
}

// ─── Dynamic Lockout ───────────────────────────────────────────────────────

/// Compute dynamic lockout fraction (PRECISION-scaled).
///
/// Two signals:
///   1. Pool imbalance (60% weight) — lopsided = likely known outcome
///   2. Participation depth (40% weight) — few = easier to manipulate
pub fn compute_lockout(pools: &[u64; MAX_SIDES], num_sides: u8, total_pos: u32) -> u64 {
    let total_pool = sum_pools(pools, num_sides);
    if total_pool == 0 {
        return MIN_LOCKOUT;
    }

    // Binary: imbalance between side 0 and side 1
    // Multi-option: max imbalance between any pair (simplified: largest vs rest)
    let imbalance = if num_sides == 2 {
        let diff = if pools[0] > pools[1] {
            pools[0] - pools[1]
        } else {
            pools[1] - pools[0]
        };
        // imbalance = diff * PRECISION / total_pool
        (diff as u128 * PRECISION as u128 / total_pool as u128) as u64
    } else {
        // For multi-option: use largest pool vs total for simplicity
        let max_pool = pools[..num_sides as usize].iter().copied().max().unwrap_or(0);
        let rest = total_pool.saturating_sub(max_pool);
        let diff = if max_pool > rest { max_pool - rest } else { rest - max_pool };
        (diff as u128 * PRECISION as u128 / total_pool as u128) as u64
    };

    // depth_factor = min(PRECISION, total_pos * PRECISION / MIN_HEALTHY_POSITIONS)
    let depth_factor = ((total_pos as u128 * PRECISION as u128) / MIN_HEALTHY_POSITIONS as u128)
        .min(PRECISION as u128) as u64;

    let range = MAX_LOCKOUT - MIN_LOCKOUT; // 200_000

    // lockout = MIN + range * (0.6 * imbalance + 0.4 * (1 - depth)) / PRECISION
    // Using integer: (6 * imbalance + 4 * (PRECISION - depth)) / 10
    let combined = 6u128 * imbalance as u128 + 4u128 * (PRECISION - depth_factor) as u128;
    let scaled = combined / 10; // now in PRECISION scale
    let lockout = MIN_LOCKOUT as u128
        + (range as u128 * scaled / PRECISION as u128);

    let result = lockout as u64;
    result.max(MIN_LOCKOUT).min(MAX_LOCKOUT)
}

// ─── Winner Determination ──────────────────────────────────────────────────

/// Determine the winning side (0-indexed) from market state.
///
/// Binary: 4-level tiebreaker (TWD → pool share → weighted pools → first deposit).
/// Multi-option: side with highest pool amount wins.
pub fn determine_winner(market: &MarketState) -> u8 {
    if market.num_sides > 2 {
        // Multi-option: highest pool amount wins
        let mut best_side: u8 = 0;
        let mut best_amount: u64 = 0;
        for i in 0..market.num_sides as usize {
            if market.pools[i] > best_amount {
                best_amount = market.pools[i];
                best_side = i as u8;
            }
        }
        return best_side;
    }

    // Binary: 4-level tiebreaker
    let twd_0 = if market.cumulative_time > 0 {
        (market.cumulative_twd_0 / market.cumulative_time as u128) as u64
    } else {
        PRECISION / 2
    };

    let half = PRECISION / 2;

    // Level 1: TWD
    if twd_0 > half {
        return 0;
    }
    if twd_0 < half {
        return 1;
    }

    // Level 2: Current pool share
    if market.pools[0] > market.pools[1] {
        return 0;
    }
    if market.pools[1] > market.pools[0] {
        return 1;
    }

    // Level 3: Weighted pools
    if market.weighted_pools[0] > market.weighted_pools[1] {
        return 0;
    }
    if market.weighted_pools[1] > market.weighted_pools[0] {
        return 1;
    }

    // Level 4: First deposit timestamp (earlier wins)
    let t0 = market.first_deposit_at[0];
    let t1 = market.first_deposit_at[1];
    if t0 > 0 && (t1 == 0 || t0 < t1) {
        return 0;
    }
    if t1 > 0 && (t0 == 0 || t1 < t0) {
        return 1;
    }

    // Ultimate fallback: side 0
    0
}

// ─── TWCD Computation ──────────────────────────────────────────────────────

/// Compute winner_payout_share from TWCD formula.
///
/// Multi-option: PRECISION (winner takes all, no TWCD swing).
/// Binary:
///   swing = |final_winner_share - twd_winner|
///   dominance = max(0, PRECISION - 1.5 * swing)
///   wps = PRECISION/2 + dominance/2
///   Safety floor: wps > winner_fraction + 1%
pub fn compute_twcd(
    winner: u8,         // 0-indexed
    twd_0: u64,         // PRECISION-scaled
    pools: &[u64; MAX_SIDES],
    num_sides: u8,
) -> Result<u64> {
    // Multi-option: winner takes all
    if num_sides > 2 {
        return Ok(PRECISION);
    }

    let total_pool = sum_pools(pools, num_sides);
    if total_pool == 0 {
        return Ok(PRECISION / 2);
    }

    // final_winner_share = pools[winner] * PRECISION / total_pool
    let final_winner_share = (pools[winner as usize] as u128)
        .checked_mul(PRECISION as u128).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(total_pool as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;

    // twd_winner
    let twd_winner = if winner == 0 { twd_0 } else { PRECISION.saturating_sub(twd_0) };

    // swing = |final_winner_share - twd_winner|
    let swing = if final_winner_share > twd_winner {
        final_winner_share - twd_winner
    } else {
        twd_winner - final_winner_share
    };

    // dominance = max(0, PRECISION - 3 * swing / 2)
    let penalty = (DOMINANCE_NUM as u128)
        .checked_mul(swing as u128).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(DOMINANCE_DENOM as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;
    let dominance = PRECISION.saturating_sub(penalty);

    // winner_payout_share = PRECISION/2 + dominance/2
    let mut wps = PRECISION / 2 + dominance / 2;

    // Safety floor: wps must exceed winner_fraction by at least 1%
    let winner_fraction = final_winner_share;
    if wps <= winner_fraction {
        wps = PRECISION.min(winner_fraction.saturating_add(MIN_TRANSFER_BPS));
    }

    Ok(wps)
}

// ─── Payout Computation ────────────────────────────────────────────────────

/// Compute payout for a winning position. Returns (payout, fee).
pub fn compute_winner_payout(
    amount: u64,
    entry_weight: u64,
    winner_side: u8,
    pools: &[u64; MAX_SIDES],
    weighted_pools: &[u128; MAX_SIDES],
    winner_payout_share: u64,
    total_distributable: u64,
) -> Result<(u64, u64)> {
    let ws = winner_side as usize;

    // winner_pool_amount = total_distributable * winner_payout_share / PRECISION
    let winner_pool = (total_distributable as u128)
        .checked_mul(winner_payout_share as u128).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(PRECISION as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;

    let winner_investment = pools[ws];
    let total_winner_weighted = weighted_pools[ws];

    if total_winner_weighted == 0 {
        return Ok((0, 0));
    }

    let profit_pool = winner_pool.saturating_sub(winner_investment);

    if profit_pool > 0 {
        // weighted_share = (amount * weight) — position's contribution
        let my_weighted = (amount as u128)
            .checked_mul(entry_weight as u128).ok_or(error!(ChubiError::MathOverflow))?;

        // gross_profit = profit_pool * my_weighted / total_winner_weighted
        let gross_profit = (profit_pool as u128)
            .checked_mul(my_weighted).ok_or(error!(ChubiError::MathOverflow))?
            .checked_div(total_winner_weighted).ok_or(error!(ChubiError::MathOverflow))? as u64;

        // fee = gross_profit * PROTOCOL_FEE_BPS / BPS_SCALE
        let fee = (gross_profit as u128)
            .checked_mul(PROTOCOL_FEE_BPS as u128).ok_or(error!(ChubiError::MathOverflow))?
            .checked_div(BPS_SCALE as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;

        let payout = amount
            .checked_add(gross_profit).ok_or(error!(ChubiError::MathOverflow))?
            .checked_sub(fee).ok_or(error!(ChubiError::MathOverflow))?;

        Ok((payout, fee))
    } else {
        // Rare edge: winner pool < winner investment. Scale down proportionally.
        if winner_investment == 0 {
            return Ok((0, 0));
        }
        let payout = (winner_pool as u128)
            .checked_mul(amount as u128).ok_or(error!(ChubiError::MathOverflow))?
            .checked_div(winner_investment as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;
        Ok((payout, 0))
    }
}

/// Compute payout for a losing position. Returns payout (fee is always 0).
pub fn compute_loser_payout(
    amount: u64,
    entry_weight: u64,
    loser_side: u8,
    weighted_pools: &[u128; MAX_SIDES],
    winner_payout_share: u64,
    total_distributable: u64,
) -> Result<u64> {
    let ls = loser_side as usize;
    let total_loser_weighted = weighted_pools[ls];

    if total_loser_weighted == 0 {
        return Ok(0);
    }

    // loser_pool = total_distributable * (PRECISION - winner_payout_share) / PRECISION
    let loser_share = PRECISION.saturating_sub(winner_payout_share);
    let loser_pool = (total_distributable as u128)
        .checked_mul(loser_share as u128).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(PRECISION as u128).ok_or(error!(ChubiError::MathOverflow))? as u64;

    // raw = loser_pool * (amount * weight) / total_loser_weighted
    let my_weighted = (amount as u128)
        .checked_mul(entry_weight as u128).ok_or(error!(ChubiError::MathOverflow))?;
    let raw = (loser_pool as u128)
        .checked_mul(my_weighted).ok_or(error!(ChubiError::MathOverflow))?
        .checked_div(total_loser_weighted).ok_or(error!(ChubiError::MathOverflow))? as u64;

    // Cap: losers never profit
    Ok(raw.min(amount))
}

// ─── Withdrawal Penalty ────────────────────────────────────────────────────

/// Compute withdrawal penalty in basis points based on fraction of time elapsed.
pub fn compute_withdrawal_penalty(elapsed: i64, duration: i64) -> u64 {
    if duration <= 0 {
        return 3000;
    }
    // fraction_elapsed = elapsed * PRECISION / duration
    let frac = (elapsed as u128 * PRECISION as u128 / duration as u128) as u64;

    if frac < 100_000 {
        500 // 5%
    } else if frac < 300_000 {
        1000 // 10%
    } else if frac < 500_000 {
        1500 // 15%
    } else if frac < 750_000 {
        2500 // 25%
    } else {
        3000 // 30%
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Entry Weight ──

    #[test]
    fn weight_at_start() {
        // Full time remaining → max weight
        let w = compute_entry_weight(PRECISION, 86400).unwrap();
        assert_eq!(w, PRECISION); // 1.0x
    }

    #[test]
    fn weight_at_end() {
        // No time remaining → floor
        let w = compute_entry_weight(0, 86400).unwrap();
        assert_eq!(w, MIN_WEIGHT_FLOOR); // 0.3x
    }

    #[test]
    fn weight_midpoint_short_market() {
        // 10-min market, 50% remaining, exponent=1 (linear)
        // weight = 300_000 + 700_000 * 500_000 / 1_000_000 = 650_000
        let w = compute_entry_weight(500_000, 600).unwrap();
        assert_eq!(w, 650_000);
    }

    #[test]
    fn weight_midpoint_medium_market() {
        // 2-hour market, 50% remaining, exponent=2 (quadratic)
        // frac^2/P = 500000^2/1000000 = 250000
        // weight = 300_000 + 700_000 * 250000 / 1000000 = 475_000
        let w = compute_entry_weight(500_000, 7200).unwrap();
        assert_eq!(w, 475_000);
    }

    #[test]
    fn weight_midpoint_long_market() {
        // 24-hour market, 50% remaining, exponent=2 (quadratic)
        // Unified: all markets >= 30min use the same quadratic curve.
        // frac^2/P = 500000^2/1000000 = 250000
        // weight = 300_000 + 700_000 * 250000 / 1000000 = 475_000
        let w = compute_entry_weight(500_000, 86400).unwrap();
        assert_eq!(w, 475_000);
    }

    #[test]
    fn weight_midpoint_week_market() {
        // 7-day market, 50% remaining — same curve as the 24h case.
        // Regression for the cubic-cliff bug: pre-unification this returned 387_500,
        // which (combined with the frontend still showing the quadratic value) meant
        // users deposited expecting ~0.475x weight and received ~0.388x on-chain.
        let w = compute_entry_weight(500_000, 604_800).unwrap();
        assert_eq!(w, 475_000);
    }

    #[test]
    fn weight_clamped_above() {
        // fraction_remaining > PRECISION should clamp to PRECISION
        let w = compute_entry_weight(2_000_000, 86400).unwrap();
        assert_eq!(w, PRECISION);
    }

    // ── Lockout ──

    #[test]
    fn lockout_empty_pool() {
        let pools = [0u64; MAX_SIDES];
        let l = compute_lockout(&pools, 2, 0);
        assert_eq!(l, MIN_LOCKOUT);
    }

    #[test]
    fn lockout_balanced_healthy() {
        // Balanced pool (imbalance=0), 10+ positions (depth=1)
        // lockout = 5% + 20% * (0 + 0) / P = 5%
        let pools = [500_000_000, 500_000_000, 0, 0, 0, 0];
        let l = compute_lockout(&pools, 2, 10);
        assert_eq!(l, MIN_LOCKOUT); // 5%
    }

    #[test]
    fn lockout_fully_imbalanced() {
        // All on one side, healthy depth
        // imbalance = PRECISION, depth=1
        // combined = 6 * 1M + 4 * 0 = 6M, /10 = 600_000
        // lockout = 50_000 + 200_000 * 600_000 / 1_000_000 = 170_000 (17%)
        let pools = [1_000_000_000, 0, 0, 0, 0, 0];
        let l = compute_lockout(&pools, 2, 10);
        assert_eq!(l, 170_000);
    }

    // ── TWCD ──

    #[test]
    fn twcd_balanced() {
        // twd_0 = 50%, winner=0 with 50% of pool
        // swing=0, dominance=PRECISION, wps = 50% + 50% = 100%
        let pools = [500, 500, 0, 0, 0, 0];
        let wps = compute_twcd(0, 500_000, &pools, 2).unwrap();
        assert_eq!(wps, PRECISION); // 100% to winners
    }

    #[test]
    fn twcd_surprise_outcome() {
        // Side 0 expected to win (twd=80%), but side 1 wins with 55%
        // twd_winner = 1M - 800k = 200k
        // final_winner_share = 550k
        // swing = |550k - 200k| = 350k
        // dominance = 1M - 3*350k/2 = 1M - 525k = 475k
        // wps = 500k + 475k/2 = 737_500
        let pools = [450, 550, 0, 0, 0, 0];
        let wps = compute_twcd(1, 800_000, &pools, 2).unwrap();
        assert_eq!(wps, 737_500);
    }

    #[test]
    fn twcd_multi_option() {
        // Multi-option: always PRECISION (winner takes all)
        let pools = [100, 200, 300, 0, 0, 0];
        let wps = compute_twcd(2, 0, &pools, 3).unwrap();
        assert_eq!(wps, PRECISION);
    }

    #[test]
    fn twcd_safety_floor() {
        // Winner has 80% of pool, twd_winner=80%, swing=0
        // dominance=PRECISION, wps=PRECISION → OK (already above fraction)
        // But test when wps would be below winner_fraction:
        // Create scenario: winner=0, pools[0]=900, pools[1]=100
        // final_winner_share = 900_000
        // twd_0 = 100_000 (side 0 dominated late)
        // twd_winner = 100_000
        // swing = |900_000 - 100_000| = 800_000
        // dominance = max(0, 1M - 3*800k/2) = max(0, 1M - 1.2M) = 0
        // wps = 500_000 + 0 = 500_000
        // winner_fraction = 900_000, wps(500k) < winner_fraction(900k)
        // Safety floor: wps = min(1M, 900_000 + 10_000) = 910_000
        let pools = [900, 100, 0, 0, 0, 0];
        let wps = compute_twcd(0, 100_000, &pools, 2).unwrap();
        assert_eq!(wps, 910_000);
    }

    // ── Winner Payout ──

    #[test]
    fn winner_payout_basic() {
        // Single winner: 100 lamports, weight 1M
        // total_distributable = 200, winner_payout_share = 700_000 (70%)
        // winner_pool = 200 * 700_000 / 1M = 140
        // profit_pool = 140 - 100 = 40
        // gross_profit = 40 * (100*1M) / (100*1M) = 40
        // fee = 40 * 200 / 10_000 = 0 (integer truncation: 0.8 → 0)
        // payout = 100 + 40 - 0 = 140
        let pools = [100, 100, 0, 0, 0, 0];
        let weighted = [100_000_000u128, 100_000_000, 0, 0, 0, 0];
        let (payout, fee) = compute_winner_payout(
            100, 1_000_000, 0, &pools, &weighted, 700_000, 200,
        ).unwrap();
        assert_eq!(payout, 140);
        assert_eq!(fee, 0); // 0.8 lamports truncated
    }

    #[test]
    fn winner_payout_with_fee() {
        // Larger amounts to see fee
        // 1_000_000 lamports, weight 1M, total_dist=2M, wps=700_000
        // winner_pool = 1_400_000
        // profit = 400_000
        // gross = 400_000
        // fee = 400_000 * 200 / 10_000 = 8_000
        // payout = 1_000_000 + 400_000 - 8_000 = 1_392_000
        let pools = [1_000_000, 1_000_000, 0, 0, 0, 0];
        let weighted = [1_000_000_000_000u128, 1_000_000_000_000, 0, 0, 0, 0];
        let (payout, fee) = compute_winner_payout(
            1_000_000, 1_000_000, 0, &pools, &weighted, 700_000, 2_000_000,
        ).unwrap();
        assert_eq!(payout, 1_392_000);
        assert_eq!(fee, 8_000);
    }

    // ── Loser Payout ──

    #[test]
    fn loser_payout_basic() {
        // Loser: 100 lamports, weight 1M
        // wps=700_000 → loser_share=300_000
        // loser_pool = 200 * 300_000 / 1M = 60
        // raw = 60 * (100*1M) / (100*1M) = 60
        // capped = min(60, 100) = 60
        let weighted = [100_000_000u128, 100_000_000, 0, 0, 0, 0];
        let payout = compute_loser_payout(
            100, 1_000_000, 1, &weighted, 700_000, 200,
        ).unwrap();
        assert_eq!(payout, 60);
    }

    #[test]
    fn loser_payout_capped() {
        // Edge: loser would get more than investment
        // This shouldn't happen with safety floor, but test the cap
        let weighted = [100_000_000u128, 100_000_000, 0, 0, 0, 0];
        let payout = compute_loser_payout(
            50, 1_000_000, 1, &weighted, 200_000, 200,
        ).unwrap();
        // loser_pool = 200 * 800_000 / 1M = 160
        // raw = 160 * (50*1M) / (100*1M) = 80
        // capped at 50
        assert_eq!(payout, 50);
    }

    // ── Withdrawal Penalty ──

    #[test]
    fn penalty_early() {
        assert_eq!(compute_withdrawal_penalty(50, 1000), 500); // 5%
    }

    #[test]
    fn penalty_mid() {
        assert_eq!(compute_withdrawal_penalty(200, 1000), 1000); // 10%
    }

    #[test]
    fn penalty_late() {
        assert_eq!(compute_withdrawal_penalty(400, 1000), 1500); // 15%
    }

    #[test]
    fn penalty_very_late() {
        assert_eq!(compute_withdrawal_penalty(600, 1000), 2500); // 25%
    }

    #[test]
    fn penalty_near_end() {
        assert_eq!(compute_withdrawal_penalty(800, 1000), 3000); // 30%
    }

    // ── Winner Determination ──

    #[test]
    fn winner_by_twd() {
        let mut market = default_market();
        market.pools = [600, 400, 0, 0, 0, 0];
        market.cumulative_twd_0 = 600_000; // twd_0 > 50%
        market.cumulative_time = 1;
        assert_eq!(determine_winner(&market), 0);
    }

    #[test]
    fn winner_by_pool_share() {
        let mut market = default_market();
        market.pools = [600, 400, 0, 0, 0, 0];
        market.cumulative_twd_0 = 500_000; // TWD tied at 50%
        market.cumulative_time = 1;
        assert_eq!(determine_winner(&market), 0); // bigger pool
    }

    #[test]
    fn winner_multi_option() {
        let mut market = default_market();
        market.num_sides = 3;
        market.pools = [100, 300, 200, 0, 0, 0];
        assert_eq!(determine_winner(&market), 1); // highest pool
    }

    fn default_market() -> MarketState {
        MarketState {
            bump: 0,
            vault_bump: 0,
            authority: Pubkey::default(),
            market_id: String::new(),
            status: crate::state::MarketStatus::Open,
            winner: 0,
            num_sides: 2,
            created_at: 0,
            resolution_duration: 86400,
            resolved_at: 0,
            total_deposited: 0,
            total_claimed: 0,
            position_count: 0,
            pools: [0; MAX_SIDES],
            weighted_pools: [0; MAX_SIDES],
            side_position_counts: [0; MAX_SIDES],
            first_deposit_at: [0; MAX_SIDES],
            penalty_pool: 0,
            cumulative_twd_0: 0,
            cumulative_time: 0,
            last_snapshot_at: 0,
            has_both_sides: false,
            winner_payout_share: 0,
            twd_0: 0,
            allow_withdrawal: false,
            enable_lockout: true,
            protocol_fee_collected: 0,
            fee_recipient: Pubkey::default(),
        }
    }
}
