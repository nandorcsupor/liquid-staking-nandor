import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { LiquidStaking } from "../target/types/liquid_staking";
import { Keypair, LAMPORTS_PER_SOL, PublicKey } from "@solana/web3.js";
import { assert } from "chai";
import {
  createAssociatedTokenAccount,
  getAccount as getTokenAccount,
  getAssociatedTokenAddress,
} from "@solana/spl-token";

describe("liquid-staking", () => {
  // Configure the client to use the local cluster
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.LiquidStaking as Program<LiquidStaking>;
  const provider = anchor.getProvider();

  // Test accounts
  let authority: Keypair;
  let user: Keypair;
  let pool: PublicKey;
  let fluidSOLMint: Keypair; // Changed to Keypair for signing
  let userFluidSOLAccount: PublicKey;
  let poolBump: number;

  let validatorVoteAccount: Keypair;
  let validatorInfo: PublicKey;
  let stakeAccount1: Keypair;

  before(async function () {
    this.timeout(60000); // 1 minute timeout
    // Check network environment
    const cluster = provider.connection.rpcEndpoint;
    const isDevnet = cluster.includes("devnet");

    if (isDevnet) {
      console.log(
        "⏭️ SKIPPING: Local tests only run locally!",
      );
      this.skip();
    }

    console.log(`🌐 Running local tests on: ${cluster}`);
    // Generate keypairs
    authority = Keypair.generate();
    user = Keypair.generate();
    fluidSOLMint = Keypair.generate();
    validatorVoteAccount = Keypair.generate();
    stakeAccount1 = Keypair.generate();

    // Airdrop SOL to test accounts
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(
        authority.publicKey,
        10 * LAMPORTS_PER_SOL,
      ),
    );
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(
        user.publicKey,
        5 * LAMPORTS_PER_SOL,
      ),
    );

    // Find PDA addresses
    [pool, poolBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("pool"), authority.publicKey.toBuffer()],
      program.programId,
    );

    // Find validator info PDA
    [validatorInfo] = PublicKey.findProgramAddressSync(
      [Buffer.from("validator"), pool.toBuffer(), Buffer.from([0])],
      program.programId,
    );
  });

  describe("1. Pool Initialization", () => {
    it("Should initialize staking pool successfully", async () => {
      const tx = await program.methods
        .initializePool()
        .accounts({
          authority: authority.publicKey,
          fluidSolMint: fluidSOLMint.publicKey,
        })
        .signers([authority, fluidSOLMint])
        .rpc();

      console.log("Initialize pool tx:", tx);

      // Verify pool state
      const poolAccount = await program.account.stakingPool.fetch(pool);
      assert.equal(
        poolAccount.authority.toString(),
        authority.publicKey.toString(),
      );
      assert.equal(poolAccount.totalSolDeposited.toNumber(), 0);
      assert.equal(poolAccount.totalFluidSolMinted.toNumber(), 0);
      assert.equal(poolAccount.exchangeRate.toNumber(), 1_000_000_000); // 1:1 ratio
      assert.equal(poolAccount.targetReserveRatio, 30);
      assert.equal(poolAccount.protocolFeeBps, 1000); // 10%
    });
  });

  describe("2. SOL Deposits", () => {
    it("Should deposit SOL and mint FluidSOL tokens", async () => {
      // Create user's FluidSOL token account
      userFluidSOLAccount = await getAssociatedTokenAddress(
        fluidSOLMint.publicKey,
        user.publicKey,
      );
      await createAssociatedTokenAccount(
        provider.connection,
        user,
        fluidSOLMint.publicKey,
        user.publicKey,
      );

      const depositAmount = 2 * LAMPORTS_PER_SOL; // 2 SOL

      const tx = await program.methods
        .depositSol(new anchor.BN(depositAmount))
        .accounts({
          user: user.publicKey,
          authority: authority.publicKey,
          fluidSolMint: fluidSOLMint.publicKey,
          userFluidSolAccount: userFluidSOLAccount,
        })
        .signers([user])
        .rpc();

      console.log("Deposit SOL tx:", tx);

      // Verify pool state
      const poolAccount = await program.account.stakingPool.fetch(pool);
      assert.equal(poolAccount.totalSolDeposited.toNumber(), depositAmount);
      assert.equal(poolAccount.totalFluidSolMinted.toNumber(), depositAmount); // 1:1 ratio initially
      assert.equal(poolAccount.liquidReserve.toNumber(), depositAmount);

      // Verify user's FluidSOL balance
      const userTokenAccount = await getTokenAccount(
        provider.connection,
        userFluidSOLAccount,
      );
      assert.equal(
        userTokenAccount.amount.toString(),
        depositAmount.toString(),
      );
    });

    it("Should reject deposits below minimum", async () => {
      const smallAmount = 500_000; // 0.0005 SOL (below 0.001 minimum)

      try {
        await program.methods
          .depositSol(new anchor.BN(smallAmount))
          .accounts({
            user: user.publicKey,
            authority: authority.publicKey,
            fluidSolMint: fluidSOLMint.publicKey,
            userFluidSolAccount: userFluidSOLAccount,
          })
          .signers([user])
          .rpc();

        assert.fail("Should have failed with minimum deposit error");
      } catch (err) {
        assert.include(err.toString(), "MinimumDeposit");
      }
    });
  });

  describe("3. SOL Withdrawals", () => {
    it("Should withdraw SOL instantly from liquid reserve", async () => {
      const withdrawAmount = 0.5 * LAMPORTS_PER_SOL; // 0.5 FluidSOL

      const userBalanceBefore = await provider.connection.getBalance(
        user.publicKey,
      );

      const tx = await program.methods
        .withdrawSol(new anchor.BN(withdrawAmount), true) // instant = true
        .accounts({
          user: user.publicKey,
          authority: authority.publicKey,
          fluidSolMint: fluidSOLMint.publicKey,
          userFluidSolAccount: userFluidSOLAccount,
        })
        .signers([user])
        .rpc();

      console.log("Withdraw SOL tx:", tx);

      // Verify user received SOL (minus 0.3% fee)
      const userBalanceAfter = await provider.connection.getBalance(
        user.publicKey,
      );
      const expectedAmount = withdrawAmount * 0.997; // 0.3% fee
      assert.approximately(
        userBalanceAfter - userBalanceBefore,
        expectedAmount,
        5000, // 5000 lamport tolerance for tx fees
      );

      // Verify FluidSOL tokens were burned
      const userTokenAccount = await getTokenAccount(
        provider.connection,
        userFluidSOLAccount,
      );
      assert.equal(
        userTokenAccount.amount.toString(),
        (1.5 * LAMPORTS_PER_SOL).toString(),
      );
    });

    it("Should fail when insufficient liquidity for instant withdrawal", async () => {
      const largeAmount = 5 * LAMPORTS_PER_SOL; // More than available

      try {
        await program.methods
          .withdrawSol(new anchor.BN(largeAmount), true)
          .accounts({
            user: user.publicKey,
            authority: authority.publicKey,
            fluidSolMint: fluidSOLMint.publicKey,
            userFluidSolAccount: userFluidSOLAccount,
          })
          .signers([user])
          .rpc();

        assert.fail("Should have failed with insufficient liquidity");
      } catch (err) {
        assert.include(err.toString(), "InsufficientLiquidity");
      }
    });
  });

  describe("4. Validator Management", () => {
    it("Should add validator to pool", async () => {
      const allocation = 50; // 50%

      const [validatorInfo] = PublicKey.findProgramAddressSync(
        [Buffer.from("validator"), pool.toBuffer(), Buffer.from([0])],
        program.programId,
      );

      const tx = await program.methods
        .addValidator(validatorVoteAccount.publicKey, allocation)
        .accounts({
          authority: authority.publicKey,
          pool: pool,
          validatorInfo: validatorInfo,
        })
        .signers([authority])
        .rpc();

      console.log("Add validator tx:", tx);

      // Verify validator was added
      const poolAccount = await program.account.stakingPool.fetch(pool);
      assert.equal(poolAccount.validatorCount, 1);

      const validatorAccount = await program.account.validatorInfo.fetch(
        validatorInfo,
      );
      assert.equal(
        validatorAccount.voteAccount.toString(),
        validatorVoteAccount.publicKey.toString(),
      );
      assert.equal(validatorAccount.allocationPercentage, allocation);
      assert.equal(validatorAccount.isActive, true);
    });

    it("Should reject unauthorized validator addition", async () => {
      const validatorVote = Keypair.generate().publicKey;
      const [validatorInfo] = PublicKey.findProgramAddressSync(
        [Buffer.from("validator"), pool.toBuffer(), Buffer.from([1])],
        program.programId,
      );

      try {
        await program.methods
          .addValidator(validatorVote, 30)
          .accounts({
            authority: user.publicKey,
            pool: pool,
            validatorInfo,
          })
          .signers([user])
          .rpc();

        assert.fail("Should have failed with unauthorized error");
      } catch (err) {
        assert.include(err.toString(), "Unauthorized");
      }
    });
  });

  describe("5. Rewards Update", () => {
    it("Should update rewards and exchange rate", async () => {
      // Get current protocol fees BEFORE adding rewards
      const poolBefore = await program.account.stakingPool.fetch(pool);
      const previousFees = poolBefore.protocolFeesEarned.toNumber();

      const rewardsEarned = 0.1 * LAMPORTS_PER_SOL; // 0.1 SOL rewards

      await program.methods
        .updateRewards(new anchor.BN(rewardsEarned))
        .accounts({
          authority: authority.publicKey,
        })
        .signers([authority])
        .rpc();

      const poolAccount = await program.account.stakingPool.fetch(pool);

      const newProtocolFee = rewardsEarned * 0.1;
      const expectedTotalFees = previousFees + newProtocolFee;

      assert.approximately(
        poolAccount.protocolFeesEarned.toNumber(),
        expectedTotalFees,
        1000,
      );

      // Exchange rate should increase
      assert.isAbove(poolAccount.exchangeRate.toNumber(), 1_000_000_000);
    });
  });

  describe("6. Protocol Fee Withdrawal", () => {
    it("Should allow authority to withdraw protocol fees", async () => {
      const poolAccount = await program.account.stakingPool.fetch(pool);
      const feesToWithdraw = poolAccount.protocolFeesEarned;

      const authorityBalanceBefore = await provider.connection.getBalance(
        authority.publicKey,
      );

      const tx = await program.methods
        .withdrawProtocolFees(feesToWithdraw)
        .accounts({
          authority: authority.publicKey,
        })
        .signers([authority])
        .rpc();

      console.log("Withdraw fees tx:", tx);

      // Verify fees were withdrawn
      const authorityBalanceAfter = await provider.connection.getBalance(
        authority.publicKey,
      );
      assert.isAbove(authorityBalanceAfter, authorityBalanceBefore);

      const poolAccountAfter = await program.account.stakingPool.fetch(pool);
      assert.equal(poolAccountAfter.protocolFeesEarned.toNumber(), 0);
    });
  });
});
