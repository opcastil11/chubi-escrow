/**
 * Scenario runs against the in-memory perp model. Mocha-style, no anchor
 * dependency — `npm run sim` to execute.
 *
 * Each scenario prints a per-epoch table so you can visualize how pools /
 * holder PnL evolve under different conditions. The `expect()`s are just
 * regression anchors — the value of this file is the printed output.
 */
import { expect } from "chai";
import {
  PerpSim,
  lam,
  sol,
  EPOCH_SECS,
  PRECISION,
  MIN_WEIGHT_FLOOR,
  CRANKER_REBATE_BPS,
} from "./perp";

const HOUR = EPOCH_SECS;
const DAY = HOUR * 24n;

function pad(s: string, w: number): string {
  return s.length >= w ? s.slice(0, w) : s + " ".repeat(w - s.length);
}

describe("perp sim", () => {
  describe("R3a — late-entrant dilution", () => {
    it("late entrant on the winning side can still lose principal", () => {
      // Alice opens market on side A, deposits 5 SOL. After a month, Bob
      // joins side A with 5 SOL. Side B is empty so no funding fires. Both
      // exit immediately after Bob's deposit.
      const m = new PerpSim(0n);
      const alice = m.deposit("alice", 0, lam(5));
      m.advance(30n * DAY);
      const bob = m.deposit("bob", 0, lam(5));

      const aliceExit = m.exit(alice.id);
      // Re-snapshot for bob (Alice's exit drained side A by her gross).
      const bobExit = m.exit(bob.id);

      console.log("\n[R3a] alice (early) vs bob (late, 30d later) same side:");
      console.log(
        `  alice principal=${sol(alice.amount)} SOL, weight=${
          alice.entryWeight
        }, payout=${sol(aliceExit.netPayout)} (pnl ${aliceExit.pnlBps} bps)`
      );
      console.log(
        `  bob   principal=${sol(bob.amount)} SOL, weight=${
          bob.entryWeight
        }, payout=${sol(bobExit.netPayout)} (pnl ${bobExit.pnlBps} bps)`
      );

      // With R3b floor 0.9x, alice's weight=1.0x, bob's ≈0.98x. Alice should
      // get slightly more, bob slightly less. Without floor it'd be worse.
      expect(aliceExit.pnlBps > bobExit.pnlBps).to.equal(true);
      // Bob should lose at most ~1% (R3b cap) even though side B never paid.
      expect(bobExit.pnlBps > -200).to.equal(true);
    });

    it("late entrant on WINNING side after funding can still lose vs early holder", () => {
      // Side A: alice 10 SOL at t=0
      // Side B: charlie 10 SOL at t=0
      // 30 days of cranking — imbalance grows favoring A? Actually starts
      // balanced, so we need to push side A first.
      const m = new PerpSim(0n);
      m.deposit("alice", 0, lam(10));
      m.deposit("charlie", 1, lam(10));
      m.advance(1n * DAY); // give some time before bob joins
      // Push side A imbalance
      m.deposit("dan", 0, lam(20));

      // Crank for 30 days
      for (let i = 0; i < 24 * 30; i++) {
        m.advance(HOUR);
        try {
          m.crankEpoch();
        } catch {}
      }
      // Now bob joins the winning side late
      const bob = m.deposit("bob", 0, lam(5));
      const bobExit = m.exit(bob.id);

      console.log("\n[R3a] late entrant joins winning side after 30d funding:");
      console.log(
        `  pools after 30d funding: A=${sol(m.pools[0])}, B=${sol(m.pools[1])}`
      );
      console.log(
        `  bob deposit at t=30d (weight=${bob.entryWeight}), gross=${sol(
          bobExit.gross
        )}, net=${sol(bobExit.netPayout)}, pnl=${bobExit.pnlBps} bps`
      );
    });
  });

  describe("R6 — death spiral", () => {
    it("80/20 imbalance drains the minority pool over time", () => {
      const m = new PerpSim(0n);
      m.deposit("a", 0, lam(80));
      m.deposit("b", 1, lam(20));

      console.log("\n[R6] 80/20 split, no new deposits, cranking hourly:");
      console.log(
        `  ${pad("epoch", 6)} ${pad("poolA", 14)} ${pad("poolB", 14)} ${pad(
          "imbBps",
          8
        )} ${pad("fundingLam", 14)} rebateLam`
      );

      let lastB = m.pools[1];
      for (let i = 0; i < 24 * 7; i++) {
        m.advance(HOUR);
        const r = m.crankEpoch();
        if (i === 0 || i === 23 || i === 24 * 3 || i === 24 * 7 - 1) {
          console.log(
            `  ${pad(String(i + 1), 6)} ${pad(sol(m.pools[0]), 14)} ${pad(
              sol(m.pools[1]),
              14
            )} ${pad(r ? r.imbalanceBps.toString() : "-", 8)} ${pad(
              r ? r.funding.toString() : "-",
              14
            )} ${r ? r.rebate.toString() : "-"}`
          );
        }
        if (r === null && m.pools[1] === 0n) {
          console.log(
            `  pool B drained at epoch ${i + 1}, no further funding fires`
          );
          break;
        }
        lastB = m.pools[1];
      }
      // Pool B should be strictly less than its starting 20 SOL.
      expect(m.pools[1] < lam(20)).to.equal(true);
    });
  });

  describe("R7 — cranker rebate", () => {
    it("cranker receives ~5% of funding lamports each epoch", () => {
      const m = new PerpSim(0n);
      m.deposit("a", 0, lam(60));
      m.deposit("b", 1, lam(40));
      m.advance(HOUR);
      const r = m.crankEpoch();
      expect(r).to.not.be.null;
      const expectedRebate = (r!.funding * CRANKER_REBATE_BPS) / 10_000n;
      console.log(
        `\n[R7] 60/40 split, funding=${r!.funding} lam, rebate=${
          r!.rebate
        } lam (expected ${expectedRebate})`
      );
      expect(r!.rebate).to.equal(expectedRebate);
      // Vault should have decreased by exactly the rebate.
      const expectedVault = lam(100) - r!.rebate;
      expect(m.vault).to.equal(expectedVault);
    });

    it("rebate clamps to vault headroom if vault somehow short", () => {
      // Drain vault manually to simulate a near-empty state.
      const m = new PerpSim(0n);
      m.deposit("a", 0, lam(60));
      m.deposit("b", 1, lam(40));
      m.vault = 1n; // force tiny headroom
      m.advance(HOUR);
      const r = m.crankEpoch();
      expect(r!.rebate).to.equal(1n);
      expect(m.vault).to.equal(0n);
    });
  });

  describe("R8 — weight floor at 1 year", () => {
    it("after REFERENCE_WINDOW all entrants get the floor weight", () => {
      const m = new PerpSim(0n);
      m.advance(366n * DAY);
      const p = m.deposit("late", 0, lam(1));
      expect(p.entryWeight).to.equal(MIN_WEIGHT_FLOOR);
      console.log(
        `\n[R8] depositor at t=366d gets weight=${p.entryWeight} (floor=${MIN_WEIGHT_FLOOR})`
      );
    });
  });

  describe("R5 — fee scaling at low vault", () => {
    it("exit fees scale proportionally when vault is short", () => {
      // This scenario doesn't trigger in the sim (sim doesn't enforce
      // vault clamp). It's primarily for documenting that the on-chain
      // contract does scale — see exit_perpetual.rs lines 91-106.
      const m = new PerpSim(0n);
      m.deposit("a", 0, lam(5));
      m.deposit("b", 1, lam(5));
      m.advance(HOUR);
      const a0Id = 0;
      const exit = m.exit(a0Id);
      console.log(
        `\n[R5] balanced market exit: gross=${sol(exit.gross)}, fee=${sol(
          exit.protocolFee + exit.creatorFee
        )}, net=${sol(exit.netPayout)}`
      );
      // No profit on balanced exit → no fees
      expect(exit.protocolFee).to.equal(0n);
    });
  });

  describe("entry weight curve", () => {
    it("matches Rust unit tests", () => {
      // t=0 → PRECISION
      const m0 = new PerpSim(0n);
      const p0 = m0.deposit("t0", 0, lam(1));
      expect(p0.entryWeight).to.equal(PRECISION);

      // t=30d → ~984_240
      const m30 = new PerpSim(0n);
      m30.advance(30n * DAY);
      const p30 = m30.deposit("t30d", 0, lam(1));
      expect(p30.entryWeight > 980_000n && p30.entryWeight < 990_000n).to.equal(
        true
      );

      // t=6mo (~half) → 925_000
      const m6 = new PerpSim(0n);
      m6.advance(182n * DAY + 12n * HOUR);
      const p6 = m6.deposit("t6mo", 0, lam(1));
      expect(p6.entryWeight > 923_000n && p6.entryWeight < 927_000n).to.equal(
        true
      );
    });
  });
});
