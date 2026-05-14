use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::PerpError;

/// Entry weight for a perpetual deposit. Same shape as the timed-market curve
/// (quadratic decay from 1.0 → MIN_WEIGHT_FLOOR over the reference window) but
/// the window is fixed at 1 year — perpetuals don't have an explicit duration.
///
/// `seconds_since_first_deposit` is measured from the market's `created_at` so
/// the curve's reference is "how late am I, relative to the perpetual launch?"
pub fn compute_entry_weight(seconds_elapsed: i64) -> Result<u64> {
    if seconds_elapsed <= 0 {
        return Ok(PRECISION);
    }
    if seconds_elapsed >= REFERENCE_WINDOW_SECS {
        return Ok(MIN_WEIGHT_FLOOR);
    }
    // fraction_remaining = (REFERENCE_WINDOW - elapsed) / REFERENCE_WINDOW
    let remaining = REFERENCE_WINDOW_SECS - seconds_elapsed;
    let frac = (remaining as u128)
        .checked_mul(PRECISION as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(REFERENCE_WINDOW_SECS as u128).ok_or(error!(PerpError::MathOverflow))?;

    // quadratic: weight = floor + (PRECISION - floor) * frac^2 / PRECISION
    let frac_sq = frac
        .checked_mul(frac).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(PRECISION as u128).ok_or(error!(PerpError::MathOverflow))?;
    let range = (PRECISION - MIN_WEIGHT_FLOOR) as u128;
    let bonus = range
        .checked_mul(frac_sq).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(PRECISION as u128).ok_or(error!(PerpError::MathOverflow))?;
    let weight = MIN_WEIGHT_FLOOR as u128 + bonus;
    Ok((weight.min(PRECISION as u128)) as u64)
}

/// Compute funding amount for the next epoch crank. Returns `(winner_side, loser_side, funding_lamports, imbalance_bps)`.
/// Funding scales with current pool imbalance — a balanced market pays 0.
/// Returns `NoFundingNeeded` when one side is empty or pools are perfectly balanced.
pub fn compute_funding(pools: &[u64; 2]) -> Result<(u8, u8, u64, u64)> {
    let total = pools[0].checked_add(pools[1]).ok_or(error!(PerpError::MathOverflow))?;
    if total == 0 || pools[0] == 0 || pools[1] == 0 {
        return err!(PerpError::NoFundingNeeded);
    }
    let (winner, loser) = if pools[0] > pools[1] { (0u8, 1u8) } else { (1u8, 0u8) };
    let diff = pools[winner as usize] - pools[loser as usize];
    if diff == 0 {
        return err!(PerpError::NoFundingNeeded);
    }
    // imbalance in basis points: (diff / total) * 10_000
    let imbalance_bps = (diff as u128)
        .checked_mul(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(total as u128).ok_or(error!(PerpError::MathOverflow))? as u64;

    // funding_bps = base * imbalance / BPS_SCALE
    let scaled_bps = (FUNDING_RATE_BPS as u128)
        .checked_mul(imbalance_bps as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))? as u64;
    if scaled_bps == 0 {
        return err!(PerpError::NoFundingNeeded);
    }
    let funding = (pools[loser as usize] as u128)
        .checked_mul(scaled_bps as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(BPS_SCALE as u128).ok_or(error!(PerpError::MathOverflow))? as u64;
    if funding == 0 {
        return err!(PerpError::NoFundingNeeded);
    }
    Ok((winner, loser, funding, imbalance_bps))
}

/// Fair value of an exiting perpetual position at the current pool snapshot.
/// `gross = (amount × entry_weight) / weighted_pools[side] × pools[side]`.
pub fn compute_exit_value(
    amount: u64,
    entry_weight: u64,
    side: u8,
    pools: &[u64; 2],
    weighted_pools: &[u128; 2],
) -> Result<u64> {
    let s = side as usize;
    if weighted_pools[s] == 0 {
        return Ok(0);
    }
    let my_weighted = (amount as u128)
        .checked_mul(entry_weight as u128).ok_or(error!(PerpError::MathOverflow))?;
    let value = my_weighted
        .checked_mul(pools[s] as u128).ok_or(error!(PerpError::MathOverflow))?
        .checked_div(weighted_pools[s]).ok_or(error!(PerpError::MathOverflow))?;
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_at_t0_is_max() {
        assert_eq!(compute_entry_weight(0).unwrap(), PRECISION);
    }

    #[test]
    fn weight_at_window_end_is_floor() {
        assert_eq!(compute_entry_weight(REFERENCE_WINDOW_SECS).unwrap(), MIN_WEIGHT_FLOOR);
    }

    #[test]
    fn weight_at_midpoint_is_quadratic() {
        // half remaining → frac = 0.5 → frac^2 = 0.25 → weight = 0.9 + 0.1 * 0.25 = 0.925
        let w = compute_entry_weight(REFERENCE_WINDOW_SECS / 2).unwrap();
        assert_eq!(w, 925_000);
    }

    #[test]
    fn weight_one_month_in() {
        // 30d / 365d = 0.0822 elapsed → 0.9178 remaining → 0.9178^2 ≈ 0.8424
        // weight = 0.9 + 0.1 × 0.8424 ≈ 0.9842 → ~984_240
        let w = compute_entry_weight(30 * 86_400).unwrap();
        assert!(w > 980_000 && w < 990_000, "expected ~984k, got {}", w);
    }

    #[test]
    fn funding_balanced_errors() {
        assert!(compute_funding(&[1_000, 1_000]).is_err());
    }

    #[test]
    fn funding_one_sided_errors() {
        assert!(compute_funding(&[1_000, 0]).is_err());
        assert!(compute_funding(&[0, 1_000]).is_err());
    }

    #[test]
    fn funding_lopsided_pays_winner() {
        // 80/20 split, total 1000 lamports, base 10 bps, imbalance 6000 bps
        // scaled_bps = 10 * 6000 / 10000 = 6 bps → funding = 200 * 6 / 10_000 = 0
        // Need bigger amounts to see non-zero funding due to integer truncation.
        let (winner, loser, funding, imb) = compute_funding(&[80_000_000, 20_000_000]).unwrap();
        assert_eq!(winner, 0);
        assert_eq!(loser, 1);
        assert_eq!(imb, 6_000); // (60M / 100M) * 10_000 = 6000 bps
        // funding = 20M * (10 * 6000 / 10000) / 10000 = 20M * 6 / 10000 = 12_000
        assert_eq!(funding, 12_000);
    }

    #[test]
    fn exit_value_matches_pool_share() {
        // I deposited 100 with weight 1.0 on side 0; pool is 150 (got funding).
        // weighted_pools[0] = 100 * 1.0 = 100 (only depositor)
        // value = 100 * 1.0 * 150 / 100 = 150
        let value = compute_exit_value(100, PRECISION, 0, &[150, 50], &[100_000_000, 50_000_000]).unwrap();
        assert_eq!(value, 150);
    }

    #[test]
    fn exit_value_dilutes_with_more_holders() {
        // I deposited 100 weight 1.0; another depositor also 100 weight 1.0; pool = 200 (no funding yet)
        // weighted_pools[0] = 200_000_000; my share = 100_000_000 / 200_000_000 = 50%
        // value = 100 × 1 × 200 / 200 = 100. Even.
        let value = compute_exit_value(100, PRECISION, 0, &[200, 0], &[200_000_000, 0]).unwrap();
        assert_eq!(value, 100);
    }
}
