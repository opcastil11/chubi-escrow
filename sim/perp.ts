/**
 * Offline TypeScript model of chubi-perp accounting.
 *
 * Mirrors `programs/chubi-perp/src/math.rs` + the state transitions in
 * `deposit.rs`, `crank_epoch.rs`, and `exit_perpetual.rs`. Lets us iterate on
 * R3a (principal/funding split) and R6 (death spiral) designs without
 * recompiling Rust or redeploying to devnet.
 *
 * Uses BigInt to mirror u64/u128 wrap semantics exactly. All amounts in
 * lamports.
 */

// ─── Constants (kept in sync with constants.rs) ─────────────────────────────
export const PRECISION = 1_000_000n;
export const BPS_SCALE = 10_000n;
export const PROTOCOL_FEE_BPS = 200n;
export const CREATOR_FEE_BPS = 50n;
export const MIN_WEIGHT_FLOOR = 900_000n;
export const MIN_DEPOSIT_LAMPORTS = 20_000_000n; // 0.02 SOL
export const REFERENCE_WINDOW_SECS = 365n * 86_400n;
export const EPOCH_SECS = 3_600n;
export const FUNDING_RATE_BPS = 10n;
export const CRANKER_REBATE_BPS = 500n;

export const LAMPORTS_PER_SOL = 1_000_000_000n;
export const lam = (sol: number): bigint =>
  BigInt(Math.round(sol * Number(LAMPORTS_PER_SOL)));
export const sol = (lamports: bigint): string =>
  (Number(lamports) / Number(LAMPORTS_PER_SOL)).toFixed(6);

// ─── Pure math (mirrors math.rs) ────────────────────────────────────────────
export function computeEntryWeight(secondsElapsed: bigint): bigint {
  if (secondsElapsed <= 0n) return PRECISION;
  if (secondsElapsed >= REFERENCE_WINDOW_SECS) return MIN_WEIGHT_FLOOR;
  const remaining = REFERENCE_WINDOW_SECS - secondsElapsed;
  const frac = (remaining * PRECISION) / REFERENCE_WINDOW_SECS;
  const fracSq = (frac * frac) / PRECISION;
  const range = PRECISION - MIN_WEIGHT_FLOOR;
  const bonus = (range * fracSq) / PRECISION;
  const w = MIN_WEIGHT_FLOOR + bonus;
  return w > PRECISION ? PRECISION : w;
}

export type FundingResult =
  | { kind: "none"; reason: "empty" | "one-sided" | "balanced" | "zero-after-scaling" }
  | { kind: "ok"; winner: 0 | 1; loser: 0 | 1; funding: bigint; imbalanceBps: bigint };

export function computeFunding(pools: [bigint, bigint]): FundingResult {
  const total = pools[0] + pools[1];
  if (total === 0n) return { kind: "none", reason: "empty" };
  if (pools[0] === 0n || pools[1] === 0n)
    return { kind: "none", reason: "one-sided" };
  const winner: 0 | 1 = pools[0] > pools[1] ? 0 : 1;
  const loser: 0 | 1 = winner === 0 ? 1 : 0;
  const diff = pools[winner] - pools[loser];
  if (diff === 0n) return { kind: "none", reason: "balanced" };
  const imbalanceBps = (diff * BPS_SCALE) / total;
  const scaledBps = (FUNDING_RATE_BPS * imbalanceBps) / BPS_SCALE;
  if (scaledBps === 0n) return { kind: "none", reason: "zero-after-scaling" };
  const funding = (pools[loser] * scaledBps) / BPS_SCALE;
  if (funding === 0n) return { kind: "none", reason: "zero-after-scaling" };
  return { kind: "ok", winner, loser, funding, imbalanceBps };
}

export function computeExitValue(
  amount: bigint,
  entryWeight: bigint,
  side: 0 | 1,
  pools: [bigint, bigint],
  weightedPools: [bigint, bigint]
): bigint {
  if (weightedPools[side] === 0n) return 0n;
  const myWeighted = amount * entryWeight;
  return (myWeighted * pools[side]) / weightedPools[side];
}

// ─── Simulation harness ─────────────────────────────────────────────────────
export interface SimPosition {
  id: number;
  maker: string;
  side: 0 | 1;
  amount: bigint;
  entryWeight: bigint;
  createdAt: bigint;
}

export interface ExitResult {
  positionId: number;
  gross: bigint;
  principal: bigint;
  profit: bigint;
  protocolFee: bigint;
  creatorFee: bigint;
  netPayout: bigint;
  /** Profit/loss relative to principal, in basis points. */
  pnlBps: number;
}

export interface CrankResult {
  funding: bigint;
  toWinner: bigint;
  rebate: bigint;
  imbalanceBps: bigint;
  winnerSide: 0 | 1;
}

/**
 * In-memory model of one perp market. State transitions match the on-chain
 * program 1:1 (with the rebate-clamp shortcut: assumes vault never short).
 */
export class PerpSim {
  pools: [bigint, bigint] = [0n, 0n];
  weightedPools: [bigint, bigint] = [0n, 0n];
  /** Vault balance, lamports. Mirrors what's actually transferable. */
  vault: bigint = 0n;
  protocolFeeCollected: bigint = 0n;
  creatorFeeCollected: bigint = 0n;
  cumulativeFunding: bigint = 0n;
  cumulativeRebate: bigint = 0n;
  positions = new Map<number, SimPosition>();
  closed: { pos: SimPosition; result: ExitResult }[] = [];
  nextId = 0;
  isClosed = false;

  createdAt: bigint;
  lastEpochAt: bigint;

  constructor(public now: bigint = 0n) {
    this.createdAt = now;
    this.lastEpochAt = now;
  }

  advance(seconds: bigint): void {
    this.now += seconds;
  }

  deposit(maker: string, side: 0 | 1, amount: bigint): SimPosition {
    if (this.isClosed) throw new Error("MarketClosed");
    if (amount < MIN_DEPOSIT_LAMPORTS) throw new Error("DepositTooSmall");
    const elapsed = this.now - this.createdAt;
    const weight = computeEntryWeight(elapsed);
    const pos: SimPosition = {
      id: this.nextId++,
      maker,
      side,
      amount,
      entryWeight: weight,
      createdAt: this.now,
    };
    this.positions.set(pos.id, pos);
    this.pools[side] += amount;
    this.weightedPools[side] += amount * weight;
    this.vault += amount;
    return pos;
  }

  crankEpoch(): CrankResult | null {
    if (this.isClosed) throw new Error("MarketClosed");
    if (this.now - this.lastEpochAt < EPOCH_SECS)
      throw new Error("EpochNotElapsed");

    const f = computeFunding(this.pools);
    if (f.kind === "none") {
      // crank still bumps last_epoch_at on the chain? No — it errors out.
      // Match the on-chain behavior: nothing changes.
      return null;
    }
    const rebate = (f.funding * CRANKER_REBATE_BPS) / BPS_SCALE;
    const actualRebate = rebate <= this.vault ? rebate : this.vault;
    const toWinner = f.funding - actualRebate;
    this.pools[f.loser] -= f.funding;
    this.pools[f.winner] += toWinner;
    this.vault -= actualRebate;
    this.cumulativeFunding += f.funding;
    this.cumulativeRebate += actualRebate;
    this.lastEpochAt = this.now;
    return {
      funding: f.funding,
      toWinner,
      rebate: actualRebate,
      imbalanceBps: f.imbalanceBps,
      winnerSide: f.winner,
    };
  }

  exit(positionId: number): ExitResult {
    const pos = this.positions.get(positionId);
    if (!pos) throw new Error("AccountNotInitialized");
    const gross = computeExitValue(
      pos.amount,
      pos.entryWeight,
      pos.side,
      this.pools,
      this.weightedPools
    );
    const profit = gross > pos.amount ? gross - pos.amount : 0n;
    const protocolFee = (profit * PROTOCOL_FEE_BPS) / BPS_SCALE;
    const creatorFee = (profit * CREATOR_FEE_BPS) / BPS_SCALE;
    const netPayout = gross - protocolFee - creatorFee;

    this.pools[pos.side] -= gross;
    this.weightedPools[pos.side] -= pos.amount * pos.entryWeight;
    this.vault -= netPayout;
    this.protocolFeeCollected += protocolFee;
    this.creatorFeeCollected += creatorFee;
    this.positions.delete(positionId);

    const pnlBps =
      pos.amount === 0n
        ? 0
        : Number(((netPayout - pos.amount) * 10_000n) / pos.amount);
    const result: ExitResult = {
      positionId: pos.id,
      gross,
      principal: pos.amount,
      profit,
      protocolFee,
      creatorFee,
      netPayout,
      pnlBps,
    };
    this.closed.push({ pos, result });
    return result;
  }

  /** Peek at gross value without exiting (used by /book preview). */
  previewGross(positionId: number): bigint {
    const pos = this.positions.get(positionId);
    if (!pos) throw new Error("AccountNotInitialized");
    return computeExitValue(
      pos.amount,
      pos.entryWeight,
      pos.side,
      this.pools,
      this.weightedPools
    );
  }

  snapshot(): {
    poolA: string;
    poolB: string;
    weightedA: string;
    weightedB: string;
    vault: string;
    cumFunding: string;
    cumRebate: string;
    activePositions: number;
  } {
    return {
      poolA: sol(this.pools[0]),
      poolB: sol(this.pools[1]),
      weightedA: sol(this.weightedPools[0] / PRECISION),
      weightedB: sol(this.weightedPools[1] / PRECISION),
      vault: sol(this.vault),
      cumFunding: sol(this.cumulativeFunding),
      cumRebate: sol(this.cumulativeRebate),
      activePositions: this.positions.size,
    };
  }
}
