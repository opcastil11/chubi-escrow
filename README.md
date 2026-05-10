<div align="center">

<img src="assets/logo-full.svg" alt="Chubi.fun" width="360"/>

### **On-chain conviction markets on Solana**

*Bet with conviction, not just capital. Time-weighted dominance decides the winner — and how much they take home.*

[![Solana](https://img.shields.io/badge/Solana-Devnet-14F195?style=flat-square&logo=solana&logoColor=black)](https://solana.com)
[![Anchor](https://img.shields.io/badge/Anchor-0.32.1-512BD4?style=flat-square)](https://www.anchor-lang.com)
[![Rust](https://img.shields.io/badge/Rust-1.89.0-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-29%20unit%20%2B%2016%20integration-b6fe08?style=flat-square)](#development)
[![Status](https://img.shields.io/badge/status-not%20audited-orange?style=flat-square)](#security-notes)

[**Watch demo →**](https://vimeo.com/1190750180?share=copy&fl=sv&fe=ci) · [**Website →**](https://chubi.fun) · [**Program on Solscan →**](https://solscan.io/account/DUigKbzHrJTdEmAstpisqV1cQT951kkHk4PdMB5i4BbJ?cluster=devnet)

<a href="https://vimeo.com/1190750180?share=copy&fl=sv&fe=ci">
  <img src="assets/hero-yes-no.png" alt="Chubi conviction market — make your finances move" width="640"/>
</a>

<sub>▶ Click the image to watch the product demo on Vimeo</sub>

</div>

---

## What is Chubi?

Chubi is a **conviction market** protocol: a two-sided pool (`A` vs `B`) where users deposit SOL on the side they believe in. Unlike a classical prediction market, the payout is not a fixed odds split — it's computed from **time-weighted conviction dominance (TWCD)**: how strongly *and how consistently* the winning side led throughout the market's lifetime.

Early conviction beats late piling-on. Every deposit is weighted by *when* it landed, and every block of dominance is integrated into the final payout. All of this is computed **fully on-chain** — there is no off-chain settlement and no trusted oracle for resolution.

This repository contains the Anchor program that powers it.

## Why this design

| Problem with classic AMM/prediction markets | What Chubi does instead |
|---|---|
| Snipers can wait until the last block to back the obvious winner and still get paid the same as early believers. | **Quadratic entry-weight decay** (`0.3 + 0.7·t²`) — late deposits earn a fraction of an early deposit's share. |
| A momentary majority on resolution day decides everything; the rest of the market history is ignored. | **Time-Weighted Dominance** is integrated incrementally across the whole market lifetime. |
| Off-chain resolution requires a trusted operator. | **Permissionless `resolve_market`** — anyone can crank after expiry, math is on-chain. |
| Sandwich-style "click weight ≠ landed weight" griefing. | **Slippage guard** on `deposit(side, amount, min_weight)` — reverts with `WeightSlippage` if drift exceeds the user's tolerance. |
| No honest exit before resolution. | **5-tier withdrawal penalty** (5%–30%) — exit early, pay the appropriate fee. |
| Market creators have no incentive to launch interesting markets. | **0.5% creator commission** on winner profits — accrues to a side-car PDA, claimable any time by the creator wallet. |

---

## Program

<table>
<tr><td><b>Program ID</b></td><td><code>DUigKbzHrJTdEmAstpisqV1cQT951kkHk4PdMB5i4BbJ</code></td></tr>
<tr><td><b>Network</b></td><td>Solana Devnet</td></tr>
<tr><td><b>Framework</b></td><td>Anchor 0.32.1</td></tr>
<tr><td><b>Rust toolchain</b></td><td>1.89.0</td></tr>
<tr><td><b>Account sizes</b></td><td>MarketState ~476 B · PositionState ~116 B · CreatorAccount ~57 B</td></tr>
</table>

### Instructions (11)

| # | Instruction | Caller | Purpose |
|---|---|---|---|
| 1 | `create_market` | Authority | Init market, vault, and creator side-car PDAs `(…, creator: Pubkey)` — pass `Pubkey::default()` for anonymous markets |
| 2 | `deposit` | User | Deposit SOL `(side, amount, min_weight)`; entry weight computed on-chain with slippage guard |
| 3 | `resolve_market` | Anyone | Permissionless after expiry — TWD winner + TWCD computed on-chain |
| 4 | `admin_resolve` | Authority | Force-resolve with specified winner |
| 5 | `claim_payout` | User | On-chain payout computation, SOL sent from vault; deducts 2% protocol + 0.5% creator fee from winner profit |
| 6 | `withdraw` | User | Early exit, time-based penalty (5%–30%) |
| 7 | `invalidate_market` | Authority | Mark market invalid for full refunds |
| 8 | `refund` | User | Full deposit return from invalidated market |
| 9 | `close_position` | User | Recover rent after claim/refund/withdraw |
| 10 | `collect_fees` | Authority | Sweep accumulated 2% protocol fees |
| 11 | `claim_creator_fees` | Creator | Sweep accumulated 0.5% commission from vault → creator wallet (signer must match `CreatorAccount.creator`) |

### PDA seeds

```text
market   = ["market",   market_id.as_bytes()]
vault    = ["vault",    market.key()]
position = ["position", market.key(), maker.key(), nonce.to_le_bytes()]
creator  = ["creator",  market.key()]                                    // side-car: stores creator + accumulated 0.5%
```

---

## On-chain math

| Mechanism | Formula / rule |
|---|---|
| **Entry weight** | `0.3 + 0.7 · fraction^exp` where `exp = 1` (markets <30min, linear) or `exp = 2` (≥30min, quadratic) |
| **TWD** | Incremental cumulative tracking — no snapshot array kept on-chain |
| **Winner tiebreaker** | `TWD → pool size → weighted size → first deposit timestamp` |
| **TWCD** | `dominance = max(0, 1 − 1.5·swing)` · `wps = 50% + 50% · dominance` |
| **Protocol fee** | 2% on **winner profit only** (200 bps, accumulated on `MarketState`, swept by `collect_fees`) |
| **Creator commission** | 0.5% on **winner profit only** (50 bps, accumulated on the `CreatorAccount` side-car, swept by `claim_creator_fees`). Skipped when `CreatorAccount.creator == Pubkey::default()` (anonymous / system markets). |
| **Total fees** | Up to **2.5%** on winner profit. Losers and break-even winners pay **nothing**. |
| **Lockout** | 5%–25%, scaled by side imbalance × depth |
| **Withdrawal penalty** | 5 tiers (5%, 10%, 15%, 20%, 30%) by elapsed market time |
| **Slippage guard** | `WeightSlippage` (err 6025) if computed weight `<` user-supplied `min_weight`; pass `0` to disable |

All math runs through `u128` intermediates with checked arithmetic — overflows surface as `MathOverflow` instead of silently corrupting state.

---

## Project structure

```text
programs/chubi-escrow/src/
├── lib.rs            entry point — routes 11 instructions
├── state.rs          MarketState, PositionState, CreatorAccount
├── constants.rs      protocol constants (PRECISION = 1e6, PROTOCOL_FEE = 200 bps, CREATOR_FEE = 50 bps, …)
├── math.rs           pure math + 29 unit tests
├── errors.rs         28 error variants
├── events.rs         9 Anchor events (MarketCreated · Deposited · MarketResolved · PayoutClaimed · Withdrawn · Refunded · MarketInvalidated · FeesCollected · CreatorFeesClaimed)
└── instructions/     11 instruction handlers
tests/
└── chubi-escrow.ts   16 integration tests
```

---

## Development

```bash
# Compile
anchor build

# Pure-math unit tests (29)
cargo test

# Integration tests (16) — needs Node 20+
anchor test --provider.cluster localnet

# Deploy / upgrade on devnet
anchor deploy --provider.cluster devnet
```

### Deploying an update

```bash
anchor build

solana program deploy target/deploy/chubi_escrow.so \
  --url devnet \
  --program-id target/deploy/chubi_escrow-keypair.json
```

After a successful deploy, ship the IDL to the consumer apps:

```bash
cp target/idl/chubi_escrow.json ../frontend/src/idl/chubi_escrow.json
cp target/idl/chubi_escrow.json ../backend/src/chain-relay/chubi_escrow.json
```

---

## Security notes

- **u128 + checked arithmetic** everywhere; overflow surfaces as `MathOverflow` rather than silent corruption.
- **Permissionless resolution** — anyone can crank `resolve_market` once the market is past its expiry.
- **Authority-gated** instructions: `create_market`, `admin_resolve`, `invalidate_market`, `collect_fees`.
- **Creator-gated** instruction: `claim_creator_fees` — signer must equal `CreatorAccount.creator` (otherwise reverts with `NotCreator`, err 6026).
- **Trustless vault PDA** — only the program can sign transfers out of it. Neither the authority nor a market creator can drain user funds; both can only sweep their respective accumulated fees.
- **No keypair/`.env` in this repo** — never commit signing material.
- **Not audited.** Devnet deployment only. Do not trust production funds to this code yet.

---

## Repository

- **Program:** [`opcastil11/chubi-escrow`](https://github.com/opcastil11/chubi-escrow) — this repo
- **Frontend:** Chubi.fun web app (separate repo)
- **Backend:** chain-relay + indexer (separate repo)

## License

No license file is present yet — **all rights reserved by default**. Contact the repo owner before forking, redistributing, or using in production.

---

<div align="center">

<img src="assets/chubi-isotype.png" alt="Chubi" width="64"/>

**Built with conviction.** — [chubi.fun](https://chubi.fun)

</div>
