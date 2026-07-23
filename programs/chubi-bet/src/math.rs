use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::BetError;

/// Total lamports staked across every outcome.
pub fn sum_pools(pools: &[u64; MAX_SIDES], num_sides: u8) -> u64 {
    let mut total: u64 = 0;
    for i in 0..num_sides as usize {
        total = total.saturating_add(pools[i]);
    }
    total
}

/// Implied probability of an outcome, PRECISION-scaled — the "price" the UI
/// shows in cents. In a parimutuel pool this is not a quoted price but a
/// mechanical fact: the outcome's share of the pot.
///
/// Returns 0 for an empty pot, which the caller should render as "no market
/// yet" rather than "0¢".
pub fn implied_odds(side_pool: u64, total_pool: u64) -> u64 {
    if total_pool == 0 {
        return 0;
    }
    ((side_pool as u128) * PRECISION as u128 / total_pool as u128) as u64
}

/// Parimutuel settlement for a single winning bet.
///
/// `gross = stake / winner_pool * total_pool` — the whole pot, losers
/// included, split in proportion to stake. Fees apply to PROFIT only, so a
/// bettor never gets back less than they put in, and a market where everyone
/// backed the same outcome refunds stakes with no fee at all.
///
/// Returns `(payout, protocol_fee, creator_fee)`.
pub fn parimutuel_payout(
    stake: u64,
    winner_pool: u64,
    total_pool: u64,
    creator_fee_bps: u64,
) -> Result<(u64, u64, u64)> {
    require!(winner_pool > 0, BetError::NoWinningBets);
    require!(stake <= winner_pool, BetError::MathOverflow);
    require!(total_pool >= winner_pool, BetError::MathOverflow);

    // u128 throughout: stake * total_pool overflows u64 well inside the range
    // of stakes this program accepts (≈4.3 SOL × 4.3 SOL).
    let gross = (stake as u128)
        .checked_mul(total_pool as u128).ok_or(error!(BetError::MathOverflow))?
        / winner_pool as u128;
    let gross = u64::try_from(gross).map_err(|_| error!(BetError::MathOverflow))?;

    // gross >= stake because total_pool >= winner_pool, so this cannot wrap.
    let profit = gross.saturating_sub(stake);

    let fee = ((profit as u128) * PROTOCOL_FEE_BPS as u128 / BPS_SCALE as u128) as u64;
    let creator_fee = ((profit as u128) * creator_fee_bps as u128 / BPS_SCALE as u128) as u64;

    let payout = gross
        .checked_sub(fee).ok_or(error!(BetError::MathOverflow))?
        .checked_sub(creator_fee).ok_or(error!(BetError::MathOverflow))?;

    Ok((payout, fee, creator_fee))
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SOL: u64 = 1_000_000_000;

    fn pools(v: &[u64]) -> [u64; MAX_SIDES] {
        let mut p = [0u64; MAX_SIDES];
        p[..v.len()].copy_from_slice(v);
        p
    }

    #[test]
    fn odds_are_the_pool_share() {
        assert_eq!(implied_odds(6 * SOL, 10 * SOL), 600_000); // 60¢
        assert_eq!(implied_odds(0, 10 * SOL), 0);
        assert_eq!(implied_odds(10 * SOL, 10 * SOL), PRECISION);
    }

    #[test]
    fn empty_pot_has_no_price() {
        // Guards the display path: a fresh market must not render as 0¢/100¢.
        assert_eq!(implied_odds(0, 0), 0);
    }

    #[test]
    fn sums_only_the_active_sides() {
        // Slots past num_sides are always zero, but the loop must not read
        // them anyway — a 2-outcome market must ignore slots 2..6.
        let p = pools(&[3 * SOL, 7 * SOL, 99 * SOL]);
        assert_eq!(sum_pools(&p, 2), 10 * SOL);
        assert_eq!(sum_pools(&p, 3), 109 * SOL);
    }

    #[test]
    fn even_market_doubles_the_stake_minus_fees() {
        // 1 SOL of 1 SOL on the winner, 1 SOL on the loser.
        let (payout, fee, creator) = parimutuel_payout(SOL, SOL, 2 * SOL, CREATOR_FEE_BPS).unwrap();
        // profit = 1 SOL → 2% protocol, 0.5% creator
        assert_eq!(fee, 20_000_000);
        assert_eq!(creator, 5_000_000);
        assert_eq!(payout, 2 * SOL - 20_000_000 - 5_000_000);
    }

    #[test]
    fn no_creator_means_no_creator_cut() {
        let (payout, fee, creator) = parimutuel_payout(SOL, SOL, 2 * SOL, 0).unwrap();
        assert_eq!(creator, 0);
        assert_eq!(fee, 20_000_000);
        assert_eq!(payout, 2 * SOL - 20_000_000);
    }

    #[test]
    fn one_sided_market_returns_the_stake_untouched() {
        // Everyone backed the same outcome: no profit exists, so no fee may be
        // charged. Taking a cut here would hand back less than was staked.
        let (payout, fee, creator) = parimutuel_payout(3 * SOL, 10 * SOL, 10 * SOL, CREATOR_FEE_BPS).unwrap();
        assert_eq!(payout, 3 * SOL);
        assert_eq!(fee, 0);
        assert_eq!(creator, 0);
    }

    #[test]
    fn winners_never_lose_money() {
        // Property check across a spread of pool shapes: payout >= stake.
        for winner_pool in [SOL, 3 * SOL, 17 * SOL, 500 * SOL] {
            for extra in [0, SOL, 40 * SOL, 1_000 * SOL] {
                let total = winner_pool + extra;
                let stake = winner_pool / 3 + 1;
                let (payout, _, _) = parimutuel_payout(stake, winner_pool, total, CREATOR_FEE_BPS).unwrap();
                assert!(payout >= stake, "{stake} -> {payout} (pool {winner_pool}/{total})");
            }
        }
    }

    #[test]
    fn payouts_never_exceed_the_pot() {
        // Three winners split a 10 SOL pot; truncation must leave dust behind
        // in the vault rather than overdraw it.
        let winner_pool = 3 * SOL;
        let total = 10 * SOL;
        let stakes = [SOL, SOL, SOL];
        let mut paid = 0u64;
        let mut fees = 0u64;
        for s in stakes {
            let (payout, fee, creator) = parimutuel_payout(s, winner_pool, total, CREATOR_FEE_BPS).unwrap();
            paid += payout;
            fees += fee + creator;
        }
        assert!(paid + fees <= total, "paid {paid} + fees {fees} > pot {total}");
    }

    #[test]
    fn three_way_pays_the_whole_pot_to_one_outcome() {
        // Winner-takes-all: 2 SOL on the winner out of a 12 SOL pot → 6x gross.
        let (payout, fee, _) = parimutuel_payout(2 * SOL, 2 * SOL, 12 * SOL, 0).unwrap();
        let profit = 10 * SOL;
        assert_eq!(fee, profit * 200 / 10_000);
        assert_eq!(payout, 12 * SOL - fee);
    }

    #[test]
    fn a_stake_larger_than_its_pool_is_rejected() {
        // Only reachable via corrupted state, but it would mint lamports.
        assert!(parimutuel_payout(2 * SOL, SOL, 5 * SOL, 0).is_err());
    }

    #[test]
    fn an_empty_winning_pool_is_rejected() {
        assert!(parimutuel_payout(SOL, 0, 5 * SOL, 0).is_err());
    }

    #[test]
    fn large_pools_do_not_overflow() {
        // 100k SOL each side — stake * total_pool is ~1e29, far past u64.
        let big = 100_000 * SOL;
        let (payout, _, _) = parimutuel_payout(big, big, 2 * big, CREATOR_FEE_BPS).unwrap();
        assert!(payout > big);
    }
}
