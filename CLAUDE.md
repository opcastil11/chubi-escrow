# chubi-escrow — Solana On-Chain Conviction Market

Fully on-chain Anchor program for the CHUBI conviction market protocol.
All game logic (TWCD, entry weights, payout computation, fees, lockout, withdrawal penalties) runs on-chain.

This workspace ships **two** Anchor programs:
- `programs/chubi-escrow/` — timed conviction markets (this file's main subject)
- `programs/chubi-perp/`   — perpetual conviction markets (streaming funding rate); see [chubi-perp](#chubi-perp--perpetual-conviction-markets) below

---

## Program Details

| | |
|---|---|
| **Program ID** | `DUigKbzHrJTdEmAstpisqV1cQT951kkHk4PdMB5i4BbJ` |
| **Old Program ID** | `JCaqkt1wNFVXqtjkfSZjR5kbHBs2HN8Rkb2g62N4V6Dr` (orphaned — upgrade-authority keypair lost; older: `Ar35haaiz1tu2DyVetpZ96TYvUAipJ817qMF9DQm6cZm` closed) |
| **Upgrade Authority** | `4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe` (same as protocol admin — consolidated to keep access recoverable) |
| **Program keypair** | `~/.config/solana/chubi_escrow_program.json` (back this up — needed only for initial deploy to this ID; upgrades only need the upgrade authority signer) |
| **Network** | Solana Devnet |
| **Framework** | Anchor 0.32.1 |
| **Rust toolchain** | 1.89.0 |

## Instructions (10)

| Instruction | Who | Description |
|---|---|---|
| `create_market` | Authority | Init market + vault PDAs |
| `deposit` | User | SOL deposit `(side, amount, min_weight)`; entry weight computed on-chain, reverts with `WeightSlippage` if the computed weight is below `min_weight` |
| `resolve_market` | Anyone | Permissionless after expiry — TWD winner + TWCD on-chain |
| `admin_resolve` | Authority | Force-resolve with specified winner |
| `claim_payout` | User | On-chain payout computation, SOL transferred |
| `withdraw` | User | Time-based penalty (5-30%) |
| `invalidate_market` | Authority | Mark invalid for refunds |
| `refund` | User | Full deposit return from invalid market |
| `close_position` | User | Recover rent after claim/refund/withdraw |
| `collect_fees` | Authority | Sweep accumulated 2% protocol fees |

## Project Structure

```
programs/chubi-escrow/src/
  lib.rs              -- entry point, routes 10 instructions
  state.rs            -- MarketState (~476 bytes), PositionState (~116 bytes)
  constants.rs        -- 16 protocol constants (PRECISION=1M, FEE=200bps, etc.)
  math.rs             -- pure math functions + 26 unit tests
  errors.rs           -- 25 error variants
  events.rs           -- 8 Anchor events
  instructions/       -- 10 instruction files
tests/chubi-escrow.ts -- 16 integration tests
```

## On-Chain Math

- **Entry weight**: `0.3 + 0.7 * fraction^exp` where `exp=1` (<30min, linear) or `exp=2` (>=30min, quadratic). Unified formula — the prior cubic branch for >=4h markets was removed because it drove late-entry weight to the floor before the midpoint of multi-day markets and made the off-chain analytics display inconsistent with what the contract actually paid.
- **TWD**: Incremental cumulative tracking (no snapshot array)
- **Winner**: 4-level tiebreaker (TWD → pool → weighted → first deposit)
- **TWCD**: `dominance = max(0, 1 - 1.5*swing)`, `wps = 50% + 50% * dominance`
- **Protocol fee**: 2% on winner profit only
- **Lockout**: 5-25% based on imbalance + depth
- **Withdrawal**: 5 tiers (5%-30%)
- **Slippage guard**: `deposit(side, amount, min_weight)`. If the computed entry weight is below `min_weight`, the tx reverts with `WeightSlippage` (error 6025). Pass `0` to disable. Frontend passes `displayed_weight * 0.98` so a ~2% drift between click and landing is tolerated but larger drift is caught.

## PDA Seeds

- Market: `["market", market_id.as_bytes()]`
- Vault: `["vault", market.key()]`
- Position: `["position", market.key(), maker.key(), nonce.to_le_bytes()]`

## Development

```bash
anchor build                              # compile
cargo test                                # 26 math unit tests
anchor test --provider.cluster localnet   # 16 integration tests (needs Node 20+)
anchor deploy --provider.cluster devnet   # deploy to devnet
```

## Deploying updates

```bash
# 1. Build
anchor build

# 2. Deploy (upgradeable — same authority)
solana program deploy target/deploy/chubi_escrow.so \
  --url devnet \
  --program-id target/deploy/chubi_escrow-keypair.json

# 3. Copy IDL to frontend + backend
cp target/idl/chubi_escrow.json ../frontend/src/idl/chubi_escrow.json
cp target/idl/chubi_escrow.json ../backend/src/chain-relay/chubi_escrow.json
```

## Security Notes

- All math uses u128 intermediates with checked arithmetic (MathOverflow error)
- Permissionless resolution — anyone can crank after expiry
- Only authority can: create markets, admin-resolve, invalidate, collect fees
- Vault PDA is trustless — only the program can sign transfers
- Never commit wallet keypairs or `.env` files

---

## chubi-perp — Perpetual Conviction Markets

Streaming-funding-rate sibling of chubi-escrow. No expiry, no terminal resolution: a permissionless `crank_epoch` runs hourly and moves lamports loser-pool → winner-pool scaled by current imbalance. Each holder's exit value is `(amount × entry_weight / weighted_pools[side]) × pools[side]`, so positions gain or lose value in real time as funding flows. Holders exit any time via `exit_perpetual` — no lockup, fees on profit only.

| | |
|---|---|
| **Program ID** | `JBo8FAveHuB55ZXjSeBAQL8ekaEae1btnftdNaQsjz3u` |
| **Upgrade Authority** | `4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe` (shared with chubi-escrow) |
| **Network** | Solana Devnet only (not on mainnet) |
| **Admin gate** | Hardcoded `PERPETUAL_ADMINS` in `constants.rs` — only these wallets can call `create_perp_market` |

### Instructions (7)

| Instruction | Who | Description |
|---|---|---|
| `create_perp_market` | Admin (PERPETUAL_ADMINS) | Init market + vault + creator side-car |
| `deposit(side, amount)` | User | Pick a side (0 or 1), deposit SOL ≥ MIN_DEPOSIT_LAMPORTS |
| `crank_epoch` | Anyone | Move funding loser → winner; permissionless, callable after EPOCH_SECS |
| `exit_perpetual` | Maker | Withdraw at current fair value; **closes position account, refunds rent** |
| `close_perp_market` | Authority | Stop new deposits/funding; existing holders can still exit |
| `claim_creator_fees` | Creator | Sweep accumulated 0.5% commission from side-car PDA → creator wallet |
| `collect_fees` | Authority | Sweep accumulated 2% protocol fees from vault → `fee_recipient` |

### Key Constants (`programs/chubi-perp/src/constants.rs`)

| | |
|---|---|
| `MIN_DEPOSIT_LAMPORTS` | `20_000_000` (0.02 SOL) — sized to comfortably exceed PerpPositionState rent (~0.00165 SOL) |
| `MIN_WEIGHT_FLOOR` | `900_000` (0.9x) — band-aid until R3a principal/funding split is designed |
| `REFERENCE_WINDOW_SECS` | 365 days — quadratic decay from 1.0x → 0.9x |
| `EPOCH_SECS` | 3600 — crank_epoch ratelimit |
| `FUNDING_RATE_BPS` | 10 — base, scaled by current imbalance |
| `PROTOCOL_FEE_BPS` | 200 (2% on profit at exit) |
| `CREATOR_FEE_BPS` | 50 (0.5% on profit at exit) |

### PDA Seeds

- Market: `["perp_market", market_id.as_bytes()]`
- Vault:  `["perp_vault", market.key()]`
- Creator side-car: `["perp_creator", market.key()]`
- Position: `["perp_position", market.key(), maker.key(), position_count.to_le_bytes()]`

### v1 fixes shipped 2026-05-14 (commit `5ccee99`)

| Ref | Fix | Where |
|---|---|---|
| R1 | `exit_perpetual` uses `close = maker` — closes position account in same tx, refunds rent | `instructions/exit_perpetual.rs` |
| R2 | `MIN_DEPOSIT_LAMPORTS` 0.001 → 0.02 SOL (rent + meaningful floor) | `constants.rs` |
| R3b | `MIN_WEIGHT_FLOOR` 0.3 → 0.9 (caps entry-weight dilution at ~10% instead of ~70%) | `constants.rs` |
| R4 | `collect_fees` now emits `PerpProtocolFeesCollected` and uses dedicated `NoProtocolFees` (6015) error | `instructions/collect_fees.rs` |
| R5 | When vault rent-exempt floor caps `actual_payout`, scale `gross`/fees by the same ratio so `pools[]` and fee accumulators stay consistent | `instructions/exit_perpetual.rs` |

### Open work (not shipped)

- **R3a** — separate principal from funding in exit math. The current formula multiplies `amount × entry_weight` against the whole `pools[side]`, which transfers principal between late/early holders even with zero funding flow. R3b's 0.9 floor is a band-aid. A coherent rewrite makes `entry_weight` govern only the funding distribution; principal always recovers (minus funding paid if loser side). Non-trivial accounting redesign.
- **R6** — funding rate has no escape valve (only flows loser→winner, only stops at perfectly one-sided). Markets drift toward 100/0 death spiral.
- **R7** — `crank_epoch` is permissionless but cranker gets nothing; add ~1–5% of moved funding as incentive.
- **R8** — 1-year reference window means depositors past month 12 capture almost nothing; market fossilizes.
- **R9** — no integration tests for chubi-perp yet (only `programs/chubi-perp/src/math.rs` unit tests).
- **R10** — external audit before mainnet.

### Deploying chubi-perp updates

```bash
anchor build -p chubi_perp
anchor deploy -p chubi_perp --provider.cluster devnet
# Copy IDL to consuming repos:
cp target/idl/chubi_perp.json ../chubi/conviction-market-protocol/frontend/src/idl/chubi_perp.json
cp target/idl/chubi_perp.json ../chubi/conviction-market-protocol/backend/src/chain-relay/chubi_perp.json
# Then rebuild the chubi-sync + backend containers so the IDL bundled in the
# image picks up the new event/error shapes.
```

### E2E

`/home/kai/Escritorio/PROGRAMACION/chubi/conviction-market-protocol/backend/scripts/e2e-perp-devnet.js` — creates a fresh market on devnet, verifies R1/R2/R5 + chain-sync indexing. Requires Node 20+ (`structuredClone`).
