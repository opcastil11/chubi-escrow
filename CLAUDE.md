# chubi-escrow — Solana Escrow Program

On-chain Anchor program for the CHUBI conviction market protocol.

---

## Overview

This is the Solana smart contract that handles escrow for CHUBI conviction markets.
Users deposit SOL into market vaults; the authority (backend wallet) resolves markets
and sets payouts based on TWCD (Time-Weighted Conviction Dominance).

**Parent project**: [opcastil11/chubi](https://github.com/opcastil11/chubi) (public) — frontend, backend, and protocol logic.

---

## Program Details

| | |
|---|---|
| **Program ID** | `Ar35haaiz1tu2DyVetpZ96TYvUAipJ817qMF9DQm6cZm` |
| **Authority** | `4wWXFYtmono7r4JivreNbbQQzrXKjy4CkzeeyWg1Aghe` |
| **Network** | Solana Devnet |
| **Framework** | Anchor 0.32.1 |
| **Rust toolchain** | See `rust-toolchain.toml` |

## Instructions

| Instruction | Description |
|---|---|
| `create_market` | Create a new market with vault PDA. Authority = backend wallet. |
| `deposit` | Deposit SOL into a market vault for a given side. Min 0.001 SOL. |
| `resolve_market` | Authority resolves the market, declaring a winning side. |
| `set_payout` | Authority sets per-depositor payout amounts after resolution. |
| `claim_payout` | Depositor claims their payout from the vault. |
| `invalidate_market` | Authority invalidates a market, allowing full refunds. |

## Project Structure

```
programs/chubi-escrow/src/lib.rs   # All program logic (instructions, accounts, errors)
tests/chubi-escrow.ts              # Integration tests
migrations/deploy.ts               # Anchor deploy migration
Anchor.toml                        # Anchor config (devnet)
```

## Development

```bash
# Build
anchor build

# Test (devnet)
anchor test

# Deploy to devnet
anchor deploy --provider.cluster devnet
```

## Security Notes

- Market IDs max 64 chars
- Min deposit: 0.001 SOL (1,000,000 lamports)
- Resolution duration: 600s (10min) to 604,800s (7 days)
- Only the authority wallet can resolve, set payouts, or invalidate
- Vault PDAs are derived per market — funds are isolated
- Never commit wallet keypairs or `.env` files
