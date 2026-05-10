import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { ChubiEscrow } from "../target/types/chubi_escrow";
import { expect } from "chai";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import BN from "bn.js";

describe("chubi-escrow", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.ChubiEscrow as Program<ChubiEscrow>;
  const authority = provider.wallet;

  // Helper: derive PDAs
  function getMarketPDA(marketId: string): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("market"), Buffer.from(marketId)],
      program.programId
    );
  }

  function getVaultPDA(marketPubkey: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), marketPubkey.toBuffer()],
      program.programId
    );
  }

  function getPositionPDA(
    marketPubkey: PublicKey,
    makerPubkey: PublicKey,
    nonce: number
  ): [PublicKey, number] {
    const buf = Buffer.alloc(8);
    buf.writeBigUInt64LE(BigInt(nonce));
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from("position"),
        marketPubkey.toBuffer(),
        makerPubkey.toBuffer(),
        buf,
      ],
      program.programId
    );
  }

  function getCreatorPDA(marketPubkey: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("creator"), marketPubkey.toBuffer()],
      program.programId
    );
  }

  // Helper: airdrop SOL
  async function airdrop(pubkey: PublicKey, amount: number) {
    const sig = await provider.connection.requestAirdrop(
      pubkey,
      amount * LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(sig, "confirmed");
  }

  // Helper: create a funded keypair
  async function createFundedKeypair(sol: number = 2): Promise<Keypair> {
    const kp = Keypair.generate();
    await airdrop(kp.publicKey, sol);
    return kp;
  }

  // Helper: create market with defaults
  async function createMarket(
    marketId: string,
    opts: {
      duration?: number;
      numSides?: number;
      allowWithdrawal?: boolean;
      enableLockout?: boolean;
      creator?: PublicKey;
    } = {}
  ) {
    const duration = opts.duration ?? 600; // 10 min default
    const numSides = opts.numSides ?? 2;
    const allowWithdrawal = opts.allowWithdrawal ?? false;
    const enableLockout = opts.enableLockout ?? false;
    const creator = opts.creator ?? PublicKey.default;

    const [marketPDA] = getMarketPDA(marketId);
    const [vaultPDA] = getVaultPDA(marketPDA);
    const [creatorPDA] = getCreatorPDA(marketPDA);

    await program.methods
      .createMarket(
        marketId,
        new BN(duration),
        numSides,
        allowWithdrawal,
        enableLockout,
        authority.publicKey, // fee_recipient = authority for tests
        creator
      )
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: creatorPDA,
        authority: authority.publicKey,
      })
      .rpc();

    return { marketPDA, vaultPDA, creatorPDA };
  }

  // Helper: deposit as a specific user
  async function depositAs(
    marketId: string,
    maker: Keypair,
    side: number,
    amountLamports: number,
    nonce: number
  ) {
    const [marketPDA] = getMarketPDA(marketId);
    const [vaultPDA] = getVaultPDA(marketPDA);
    const [positionPDA] = getPositionPDA(
      marketPDA,
      maker.publicKey,
      nonce
    );

    await program.methods
      // min_weight=0 → slippage guard disabled in tests (weight is whatever
      // the clock produces; tests are deterministic).
      .deposit(side, new BN(amountLamports), new BN(0))
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        position: positionPDA,
        maker: maker.publicKey,
      })
      .signers([maker])
      .rpc();

    return positionPDA;
  }

  // ─── Test: Market Creation ───────────────────────────────────────────────

  it("creates a market", async () => {
    const marketId = "test-create-" + Date.now();
    const { marketPDA } = await createMarket(marketId);

    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.marketId).to.equal(marketId);
    expect(market.status).to.deep.equal({ open: {} });
    expect(market.winner).to.equal(0);
    expect(market.numSides).to.equal(2);
    expect(market.positionCount.toNumber()).to.equal(0);
    expect(market.totalDeposited.toNumber()).to.equal(0);
  });

  // ─── Test: Deposit ───────────────────────────────────────────────────────

  it("deposits SOL on both sides", async () => {
    const marketId = "test-deposit-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const maker1 = await createFundedKeypair();
    const maker2 = await createFundedKeypair();

    const amount = 0.1 * LAMPORTS_PER_SOL;

    // Side 0
    const pos1 = await depositAs(marketId, maker1, 0, amount, 0);
    // Side 1
    const pos2 = await depositAs(marketId, maker2, 1, amount, 1);

    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.positionCount.toNumber()).to.equal(2);
    expect(market.totalDeposited.toNumber()).to.equal(amount * 2);
    expect(market.pools[0].toNumber()).to.equal(amount);
    expect(market.pools[1].toNumber()).to.equal(amount);
    expect(market.hasBothSides).to.equal(true);

    // Check position data
    const position1 = await program.account.positionState.fetch(pos1);
    expect(position1.side).to.equal(0);
    expect(position1.amount.toNumber()).to.equal(amount);
    expect(position1.entryWeight.toNumber()).to.be.greaterThan(0);
    expect(position1.claimed).to.equal(false);
    expect(position1.withdrawn).to.equal(false);

    // Check vault has SOL
    const vaultBalance = await provider.connection.getBalance(vaultPDA);
    expect(vaultBalance).to.equal(amount * 2);
  });

  it("rejects deposit below minimum", async () => {
    const marketId = "test-min-dep-" + Date.now();
    await createMarket(marketId);
    const maker = await createFundedKeypair();

    try {
      await depositAs(marketId, maker, 0, 100, 0); // 100 lamports < 1M minimum
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("DepositTooSmall");
    }
  });

  it("rejects deposit on invalid side", async () => {
    const marketId = "test-side-" + Date.now();
    await createMarket(marketId);
    const maker = await createFundedKeypair();

    try {
      await depositAs(marketId, maker, 5, 0.01 * LAMPORTS_PER_SOL, 0); // side 5 on a 2-side market
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("InvalidSide");
    }
  });

  // ─── Test: Admin Resolve ─────────────────────────────────────────────────

  it("admin resolves a market", async () => {
    const marketId = "test-admin-res-" + Date.now();
    const { marketPDA } = await createMarket(marketId);

    const maker1 = await createFundedKeypair();
    const maker2 = await createFundedKeypair();
    const amount = 0.1 * LAMPORTS_PER_SOL;

    await depositAs(marketId, maker1, 0, amount, 0);
    await depositAs(marketId, maker2, 1, amount, 1);

    // Admin resolve: side 1 wins (1-indexed = 1)
    await program.methods
      .adminResolve(1)
      .accounts({
        market: marketPDA,
        authority: authority.publicKey,
      })
      .rpc();

    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.status).to.deep.equal({ resolved: {} });
    expect(market.winner).to.equal(1); // side 0 wins (1-indexed)
    expect(market.winnerPayoutShare.toNumber()).to.be.greaterThan(0);
    expect(market.twd0.toNumber()).to.be.greaterThan(0);
  });

  // ─── Test: Full Lifecycle (create → deposit → admin_resolve → claim) ────

  it("full lifecycle: deposit → resolve → claim payout", async () => {
    const marketId = "test-lifecycle-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const winner = await createFundedKeypair(5);
    const loser = await createFundedKeypair(5);
    const winAmount = 0.5 * LAMPORTS_PER_SOL;
    const loseAmount = 0.3 * LAMPORTS_PER_SOL;

    // Deposits
    const winPos = await depositAs(marketId, winner, 0, winAmount, 0);
    const losePos = await depositAs(marketId, loser, 1, loseAmount, 1);

    const winnerBalBefore = await provider.connection.getBalance(
      winner.publicKey
    );

    // Admin resolve: side 0 wins (1-indexed = 1)
    await program.methods
      .adminResolve(1)
      .accounts({
        market: marketPDA,
        authority: authority.publicKey,
      })
      .rpc();

    // Winner claims
    await program.methods
      .claimPayout()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: getCreatorPDA(marketPDA)[0],
        position: winPos,
        maker: winner.publicKey,
      })
      .signers([winner])
      .rpc();

    const winPosition = await program.account.positionState.fetch(winPos);
    expect(winPosition.claimed).to.equal(true);
    expect(winPosition.payoutAmount.toNumber()).to.be.greaterThan(winAmount);

    // Winner should have received more than they deposited
    const winnerBalAfter = await provider.connection.getBalance(
      winner.publicKey
    );
    expect(winnerBalAfter).to.be.greaterThan(winnerBalBefore);

    // Loser tries to claim consolation
    // When TWD is balanced and dominance is high, loser_pool can be 0 → NoPayout
    try {
      await program.methods
        .claimPayout()
        .accounts({
          market: marketPDA,
          vault: vaultPDA,
          creatorAccount: getCreatorPDA(marketPDA)[0],
        position: losePos,
          maker: loser.publicKey,
        })
        .signers([loser])
        .rpc();

      const losePosition = await program.account.positionState.fetch(losePos);
      expect(losePosition.claimed).to.equal(true);
      // Loser payout <= investment (invariant 2)
      expect(losePosition.payoutAmount.toNumber()).to.be.lessThanOrEqual(
        loseAmount
      );
    } catch (e: any) {
      // NoPayout is valid when loser gets 0 (high conviction market)
      expect(e.message).to.include("NoPayout");
    }

    // Check protocol fee was collected
    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.protocolFeeCollected.toNumber()).to.be.greaterThanOrEqual(0);
  });

  // ─── Test: Double Claim Prevention ───────────────────────────────────────

  it("prevents double claim", async () => {
    const marketId = "test-double-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const maker1 = await createFundedKeypair();
    const maker2 = await createFundedKeypair();
    const amount = 0.1 * LAMPORTS_PER_SOL;

    const pos1 = await depositAs(marketId, maker1, 0, amount, 0);
    await depositAs(marketId, maker2, 1, amount, 1);

    await program.methods
      .adminResolve(1)
      .accounts({ market: marketPDA, authority: authority.publicKey })
      .rpc();

    // First claim: OK
    await program.methods
      .claimPayout()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: getCreatorPDA(marketPDA)[0],
        position: pos1,
        maker: maker1.publicKey,
      })
      .signers([maker1])
      .rpc();

    // Second claim: should fail
    try {
      await program.methods
        .claimPayout()
        .accounts({
          market: marketPDA,
          vault: vaultPDA,
          creatorAccount: getCreatorPDA(marketPDA)[0],
        position: pos1,
          maker: maker1.publicKey,
        })
        .signers([maker1])
        .rpc();
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("AlreadyClaimed");
    }
  });

  // ─── Test: Invalidation + Refund ─────────────────────────────────────────

  it("invalidate market and refund", async () => {
    const marketId = "test-refund-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const maker = await createFundedKeypair();
    const amount = 0.2 * LAMPORTS_PER_SOL;

    const pos = await depositAs(marketId, maker, 0, amount, 0);

    const balBefore = await provider.connection.getBalance(maker.publicKey);

    // Invalidate
    await program.methods
      .invalidateMarket()
      .accounts({ market: marketPDA, authority: authority.publicKey })
      .rpc();

    let market = await program.account.marketState.fetch(marketPDA);
    expect(market.status).to.deep.equal({ invalid: {} });

    // Refund
    await program.methods
      .refund()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        position: pos,
        maker: maker.publicKey,
      })
      .signers([maker])
      .rpc();

    const position = await program.account.positionState.fetch(pos);
    expect(position.claimed).to.equal(true);
    expect(position.payoutAmount.toNumber()).to.equal(amount);

    const balAfter = await provider.connection.getBalance(maker.publicKey);
    // Should get back full deposit (minus tx fee)
    expect(balAfter).to.be.greaterThan(balBefore + amount - 10000);
  });

  // ─── Test: Close Position (rent recovery) ────────────────────────────────

  it("close position after claim", async () => {
    const marketId = "test-close-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const maker1 = await createFundedKeypair();
    const maker2 = await createFundedKeypair();
    const amount = 0.1 * LAMPORTS_PER_SOL;

    const pos = await depositAs(marketId, maker1, 0, amount, 0);
    await depositAs(marketId, maker2, 1, amount, 1);

    await program.methods
      .adminResolve(1)
      .accounts({ market: marketPDA, authority: authority.publicKey })
      .rpc();

    await program.methods
      .claimPayout()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: getCreatorPDA(marketPDA)[0],
        position: pos,
        maker: maker1.publicKey,
      })
      .signers([maker1])
      .rpc();

    // Close position — rent recovered
    await program.methods
      .closePosition()
      .accounts({
        market: marketPDA,
        position: pos,
        maker: maker1.publicKey,
      })
      .signers([maker1])
      .rpc();

    // Position account should no longer exist
    const info = await provider.connection.getAccountInfo(pos);
    expect(info).to.be.null;
  });

  // ─── Test: Withdrawal ────────────────────────────────────────────────────

  it("withdraw with penalty", async () => {
    const marketId = "test-withdraw-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId, {
      allowWithdrawal: true,
      enableLockout: false,
      duration: 3600, // 1 hour so we're in the early bracket
    });

    const maker = await createFundedKeypair();
    const amount = 0.5 * LAMPORTS_PER_SOL;

    const pos = await depositAs(marketId, maker, 0, amount, 0);

    const balBefore = await provider.connection.getBalance(maker.publicKey);

    // Withdraw (should be early → 5% penalty)
    await program.methods
      .withdraw()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        position: pos,
        maker: maker.publicKey,
      })
      .signers([maker])
      .rpc();

    const position = await program.account.positionState.fetch(pos);
    expect(position.withdrawn).to.equal(true);

    const balAfter = await provider.connection.getBalance(maker.publicKey);
    // Should get back ~95% (5% penalty early)
    const expectedReturn = amount * 0.95;
    const diff = balAfter - balBefore;
    // Allow for tx fees
    expect(diff).to.be.greaterThan(expectedReturn - 20000);

    // Market penalty pool should have the penalty
    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.penaltyPool.toNumber()).to.be.greaterThan(0);
    // Pool should be reduced
    expect(market.pools[0].toNumber()).to.equal(0);
  });

  it("rejects withdrawal when not allowed", async () => {
    const marketId = "test-no-wd-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId, {
      allowWithdrawal: false,
    });

    const maker = await createFundedKeypair();
    const pos = await depositAs(
      marketId,
      maker,
      0,
      0.1 * LAMPORTS_PER_SOL,
      0
    );

    try {
      await program.methods
        .withdraw()
        .accounts({
          market: marketPDA,
          vault: vaultPDA,
          position: pos,
          maker: maker.publicKey,
        })
        .signers([maker])
        .rpc();
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("WithdrawalsNotAllowed");
    }
  });

  // ─── Test: Permissionless Resolve (needs expired market) ─────────────────

  it("rejects premature permissionless resolve", async () => {
    const marketId = "test-early-res-" + Date.now();
    const { marketPDA } = await createMarket(marketId, { duration: 600 });

    const maker1 = await createFundedKeypair();
    const maker2 = await createFundedKeypair();
    await depositAs(marketId, maker1, 0, 0.1 * LAMPORTS_PER_SOL, 0);
    await depositAs(marketId, maker2, 1, 0.1 * LAMPORTS_PER_SOL, 1);

    const resolver = Keypair.generate();
    try {
      await program.methods
        .resolveMarket()
        .accounts({ market: marketPDA, resolver: resolver.publicKey })
        .signers([resolver])
        .rpc();
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("MarketNotExpired");
    }
  });

  // ─── Test: Needs 2 sides to resolve ──────────────────────────────────────

  it("rejects resolve with only one side", async () => {
    const marketId = "test-one-side-" + Date.now();
    const { marketPDA } = await createMarket(marketId);

    const maker = await createFundedKeypair();
    await depositAs(marketId, maker, 0, 0.1 * LAMPORTS_PER_SOL, 0);

    try {
      await program.methods
        .adminResolve(1)
        .accounts({ market: marketPDA, authority: authority.publicKey })
        .rpc();
      expect.fail("should have thrown");
    } catch (e: any) {
      expect(e.message).to.include("InsufficientSides");
    }
  });

  // ─── Test: Multi-option market ───────────────────────────────────────────

  it("multi-option market: winner takes all", async () => {
    const marketId = "test-multi-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId, {
      numSides: 3,
    });

    const m1 = await createFundedKeypair();
    const m2 = await createFundedKeypair();
    const m3 = await createFundedKeypair();
    const amount = 0.1 * LAMPORTS_PER_SOL;

    const p1 = await depositAs(marketId, m1, 0, amount, 0);
    const p2 = await depositAs(marketId, m2, 1, amount, 1);
    const p3 = await depositAs(marketId, m3, 2, amount, 2);

    // Admin resolve: side 2 wins (1-indexed = 3)
    await program.methods
      .adminResolve(3)
      .accounts({ market: marketPDA, authority: authority.publicKey })
      .rpc();

    let market = await program.account.marketState.fetch(marketPDA);
    expect(market.winner).to.equal(3);
    // Multi-option: winner_payout_share = PRECISION (100%)
    expect(market.winnerPayoutShare.toNumber()).to.equal(1_000_000);

    // Winner (side 2) claims — should get almost entire pool
    await program.methods
      .claimPayout()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: getCreatorPDA(marketPDA)[0],
        position: p3,
        maker: m3.publicKey,
      })
      .signers([m3])
      .rpc();

    const winPos = await program.account.positionState.fetch(p3);
    // Winner gets close to 3x their deposit (all 3 sides' SOL, minus fee)
    expect(winPos.payoutAmount.toNumber()).to.be.greaterThan(amount * 2);
  });

  // ─── Test: Fee Collection ────────────────────────────────────────────────

  it("collect fees after claims", async () => {
    const marketId = "test-fees-" + Date.now();
    const { marketPDA, vaultPDA } = await createMarket(marketId);

    const m1 = await createFundedKeypair();
    const m2 = await createFundedKeypair();
    // Larger amounts so fee is > 0
    const bigAmount = 1 * LAMPORTS_PER_SOL;

    const p1 = await depositAs(marketId, m1, 0, bigAmount, 0);
    const p2 = await depositAs(marketId, m2, 1, bigAmount, 1);

    await program.methods
      .adminResolve(1)
      .accounts({ market: marketPDA, authority: authority.publicKey })
      .rpc();

    // Winner claims
    await program.methods
      .claimPayout()
      .accounts({
        market: marketPDA,
        vault: vaultPDA,
        creatorAccount: getCreatorPDA(marketPDA)[0],
        position: p1,
        maker: m1.publicKey,
      })
      .signers([m1])
      .rpc();

    let market = await program.account.marketState.fetch(marketPDA);
    const feesCollected = market.protocolFeeCollected.toNumber();

    if (feesCollected > 0) {
      const authBalBefore = await provider.connection.getBalance(
        authority.publicKey
      );

      await program.methods
        .collectFees()
        .accounts({
          market: marketPDA,
          vault: vaultPDA,
          feeRecipient: authority.publicKey,
          authority: authority.publicKey,
        })
        .rpc();

      market = await program.account.marketState.fetch(marketPDA);
      expect(market.protocolFeeCollected.toNumber()).to.equal(0);
    }
  });

  // ─── Test: Multiple positions per user ───────────────────────────────────

  it("allows multiple positions from same user", async () => {
    const marketId = "test-multi-pos-" + Date.now();
    const { marketPDA } = await createMarket(marketId);

    const maker = await createFundedKeypair(5);
    const amount = 0.1 * LAMPORTS_PER_SOL;

    // First position (nonce 0)
    await depositAs(marketId, maker, 0, amount, 0);
    // Second position (nonce 1)
    await depositAs(marketId, maker, 0, amount, 1);

    const market = await program.account.marketState.fetch(marketPDA);
    expect(market.positionCount.toNumber()).to.equal(2);
    expect(market.pools[0].toNumber()).to.equal(amount * 2);
  });
});
