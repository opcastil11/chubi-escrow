# chubi-escrow — Solana On-Chain Conviction Market

Fully on-chain Anchor program for the CHUBI conviction market protocol.
All game logic (TWCD, entry weights, payout computation, fees, lockout, withdrawal penalties) runs on-chain.

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
