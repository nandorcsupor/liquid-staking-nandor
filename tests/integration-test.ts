import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import {
    Keypair,
    LAMPORTS_PER_SOL,
    PublicKey,
    SYSVAR_STAKE_HISTORY_PUBKEY,
} from "@solana/web3.js";
import { assert } from "chai";
import {
    createAssociatedTokenAccount,
    getAccount as getTokenAccount,
    getAssociatedTokenAddress,
} from "@solana/spl-token";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { addAbortSignal } from "stream";
import { LiquidStaking } from "../app/src/idl/liquid_staking";

// 🌐 REAL VALIDATOR VOTE ACCOUNTS (devnet/mainnet)
const REAL_VALIDATORS = {
    devnet: [
        "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw", // Solana Foundation
        "CertusDeBmqN8ZawdkxK5kFGMwBXdudvWHYwtNgNhvLu", // Certus One
    ],
    mainnet: [
        "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2", // Solana Foundation
        "CertusDeBmqN8ZawdkxK5kFGMwBXdudvWHYwtNgNhvLu", // Certus One
        "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBkPUDuswEB", // DV8 Validator
    ],
};

describe("🚀 INTEGRATION TESTS - Real Validator Staking", () => {
    // Configure provider
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);
    const program = anchor.workspace.LiquidStaking as Program<LiquidStaking>;

    // Test accounts
    let authority: Keypair;
    let user: Keypair;
    let pool: PublicKey;
    let fluidSOLMint: Keypair;
    let userFluidSOLAccount: PublicKey;
    let validatorInfo: PublicKey;
    let stakeAccount: Keypair;
    let realValidatorVote: PublicKey;

    before(async function () {
        this.timeout(60000); // 1 minute timeout

        // Check network environment
        const cluster = provider.connection.rpcEndpoint;
        const isLocalnet = cluster.includes("localhost") ||
            cluster.includes("127.0.0.1");

        if (isLocalnet) {
            console.log(
                "⏭️ SKIPPING: Integration tests only run on devnet/mainnet",
            );
            this.skip();
            return;
        }

        console.log(`🌐 Running integration tests on: ${cluster}`);

        // Determine network and get real validator
        let validators: string[];
        if (cluster.includes("devnet")) {
            validators = REAL_VALIDATORS.devnet;
            console.log("🔧 Using DEVNET validators");
        } else {
            validators = REAL_VALIDATORS.mainnet;
            console.log("🚀 Using MAINNET validators");
        }

        // Authority = deployment wallet (has SOL)
        const authorityPath = path.join(
            os.homedir(),
            ".config",
            "solana",
            "devnet-keypair.json",
        );
        authority = Keypair.fromSecretKey(
            new Uint8Array(JSON.parse(fs.readFileSync(authorityPath, "utf8"))),
        );

        // User = saved test user (manual funded)
        user = Keypair.fromSecretKey(
            new Uint8Array(
                JSON.parse(fs.readFileSync("test-user.json", "utf8")),
            ),
        );

        const voteAccounts = await provider.connection.getVoteAccounts();
        if (voteAccounts.current.length > 0) {
            realValidatorVote = new PublicKey(
                voteAccounts.current[0].votePubkey,
            );
            console.log(
                `🏦 Using live devnet validator: ${realValidatorVote.toString()}`,
            );
        } else {
            throw new Error("No active validators found on devnet!");
        }

        // Generate new ones for this test run
        fluidSOLMint = Keypair.generate();
        stakeAccount = Keypair.generate();

        console.log(`👤 Authority: ${authority.publicKey.toString()}`);
        console.log(`👥 User: ${user.publicKey.toString()}`);
        console.log(`🏦 Using real validator: ${realValidatorVote.toString()}`);

        // Check balances
        const authBalance = await provider.connection.getBalance(
            authority.publicKey,
        );
        const userBalance = await provider.connection.getBalance(
            user.publicKey,
        );

        console.log(
            `💰 Authority balance: ${authBalance / LAMPORTS_PER_SOL} SOL`,
        );
        console.log(`💰 User balance: ${userBalance / LAMPORTS_PER_SOL} SOL`);

        if (authBalance < 3 * LAMPORTS_PER_SOL) {
            console.log("❌ Authority needs more SOL!");
            return;
        }
        if (userBalance < 3 * LAMPORTS_PER_SOL) {
            console.log("❌ User needs more SOL! Run:");
            console.log(`solana transfer ${user.publicKey.toString()} 10`);
            return;
        }

        // Find PDAs
        [pool] = PublicKey.findProgramAddressSync(
            [Buffer.from("pool")],
            program.programId,
        );

        [validatorInfo] = PublicKey.findProgramAddressSync(
            [Buffer.from("validator"), pool.toBuffer(), Buffer.from([0])],
            program.programId,
        );

        console.log(`🏊 Pool PDA: ${pool.toString()}`);
        console.log(`🎯 Validator Info PDA: ${validatorInfo.toString()}`);
    });

    describe("1. 🏊 Pool Setup", () => {
        it("Should initialize staking pool", async function () {
            this.timeout(30000);

            console.log("🚀 Initializing pool...");

            const tx = await program.methods
                .initializePool()
                .accounts({
                    authority: authority.publicKey,
                    fluidSolMint: fluidSOLMint.publicKey,
                })
                .signers([authority, fluidSOLMint])
                .rpc();

            console.log(`✅ Pool initialized! TX: ${tx}`);
            console.log("🔍 Fetching pool account...");
            console.log("Pool address:", pool.toString());

            const poolAccount2 = await program.account.stakingPool.fetch(pool);

            console.log("✅ Pool fetched successfully!");
            console.log("Pool data:", {
                authority: poolAccount2.authority?.toString(),
                totalSolDeposited: poolAccount2.totalSolDeposited?.toString(),
                validatorCount: poolAccount2.validatorCount?.toString(),
            });
            // Verify pool state
            const poolAccount = await program.account.stakingPool.fetch(pool);
            assert.equal(
                poolAccount.authority.toString(),
                authority.publicKey.toString(),
            );
            assert.equal(poolAccount.validatorCount, 0);
            assert.equal(poolAccount.totalSolDeposited.toNumber(), 0);

            console.log(
                `🎯 Pool authority: ${poolAccount.authority.toString()}`,
            );
            console.log(
                `📊 Exchange rate: ${
                    poolAccount.exchangeRate.toNumber() / 1_000_000_000
                }`,
            );
        });

        it("Should add REAL validator to pool", async function () {
            this.timeout(30000);

            console.log(
                `🎯 Adding real validator: ${realValidatorVote.toString()}`,
            );

            const allocation = 70; // 70% allocation

            const tx = await program.methods
                .addValidator(realValidatorVote, allocation)
                .accounts({
                    authority: authority.publicKey,
                    pool: pool,
                    validatorInfo: validatorInfo,
                })
                .signers([authority])
                .rpc();

            console.log(`✅ Validator added! TX: ${tx}`);

            // Verify validator was added
            const poolAccount = await program.account.stakingPool.fetch(pool);
            assert.equal(poolAccount.validatorCount, 1);

            const validatorAccount = await program.account.validatorInfo.fetch(
                validatorInfo,
            );
            assert.equal(
                validatorAccount.voteAccount.toString(),
                realValidatorVote.toString(),
            );
            assert.equal(validatorAccount.allocationPercentage, allocation);
            assert.equal(validatorAccount.isActive, true);

            console.log(
                `🎯 Validator vote account: ${validatorAccount.voteAccount.toString()}`,
            );
            console.log(
                `📊 Allocation: ${validatorAccount.allocationPercentage}%`,
            );
        });
    });

    describe("2. 💰 Deposit SOL", () => {
        it("Should deposit SOL and mint FluidSOL tokens", async function () {
            this.timeout(30000);

            console.log("💰 Depositing SOL to pool...");

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

            const depositAmount = 3 * LAMPORTS_PER_SOL; // 3 SOL

            const tx = await program.methods
                .depositSol(new anchor.BN(depositAmount))
                .accounts({
                    user: user.publicKey,
                    fluidSolMint: fluidSOLMint.publicKey,
                    userFluidSolAccount: userFluidSOLAccount,
                })
                .signers([user])
                .rpc();

            console.log(
                `✅ Deposited ${
                    depositAmount / LAMPORTS_PER_SOL
                } SOL! TX: ${tx}`,
            );

            // Verify pool state
            const poolAccount = await program.account.stakingPool.fetch(pool);
            assert.equal(
                poolAccount.totalSolDeposited.toNumber(),
                depositAmount,
            );
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

            console.log(
                `🏊 Pool total SOL: ${
                    poolAccount.totalSolDeposited.toNumber() / LAMPORTS_PER_SOL
                }`,
            );
            console.log(
                `💧 Liquid reserve: ${
                    poolAccount.liquidReserve.toNumber() / LAMPORTS_PER_SOL
                }`,
            );
            console.log(
                `🪙 User FluidSOL: ${
                    Number(userTokenAccount.amount) / LAMPORTS_PER_SOL
                }`,
            );
        });
    });

    describe("3. 🚀 REAL VALIDATOR STAKING", () => {
        it("Should stake SOL to REAL validator", async function () {
            this.timeout(60000); // 1 minute timeout

            console.log("🚀 STAKING TO REAL VALIDATOR!");

            const stakeAmount = 2 * LAMPORTS_PER_SOL; // 2 SOL
            const validatorIndex = 0;

            // Get current state BEFORE staking
            const poolBefore = await program.account.stakingPool.fetch(pool);
            const validatorBefore = await program.account.validatorInfo.fetch(
                validatorInfo,
            );

            console.log(
                `💰 Pool liquid reserve BEFORE: ${
                    poolBefore.liquidReserve.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );
            console.log(
                `🎯 Validator delegated BEFORE: ${
                    validatorBefore.totalDelegated.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );
            console.log(
                `🚀 Staking ${
                    stakeAmount / LAMPORTS_PER_SOL
                } SOL to validator...`,
            );

            // Required accounts for staking
            const stakeConfigAccount = new PublicKey(
                "StakeConfig11111111111111111111111111111111",
            );

            const clock = await program.provider.connection.getSlot();

            // PERFORM REAL STAKING! 🔥
            try {
                const tx = await program.methods
                    .stakeToValidator(
                        new anchor.BN(stakeAmount),
                        new anchor.BN(clock),
                    )
                    .accounts({
                        authority: authority.publicKey,
                        validatorInfo: validatorInfo,
                        validatorVoteAccount: realValidatorVote,
                        stakeHistory: SYSVAR_STAKE_HISTORY_PUBKEY,
                        stakeConfig: stakeConfigAccount,
                    })
                    .signers([authority])
                    .rpc();

                console.log(`🎉 REAL STAKING SUCCESSFUL! TX: ${tx}`);
            } catch (error) {
                console.log("❌ FAILED as expected!");

                throw error;
            }

            // Verify state changes AFTER staking
            const poolAfter = await program.account.stakingPool.fetch(pool);
            const validatorAfter = await program.account.validatorInfo.fetch(
                validatorInfo,
            );

            console.log(
                `💰 Pool liquid reserve AFTER: ${
                    poolAfter.liquidReserve.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );
            console.log(
                `🏗️ Pool staked balance AFTER: ${
                    poolAfter.stakedSolBalance.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );
            console.log(
                `🎯 Validator delegated AFTER: ${
                    validatorAfter.totalDelegated.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );

            // Verify accounting is correct
            assert.isTrue(
                poolAfter.liquidReserve.toNumber() <
                    poolBefore.liquidReserve.toNumber(),
                "Liquid reserve should decrease",
            );
            assert.isTrue(
                poolAfter.stakedSolBalance.toNumber() >
                    poolBefore.stakedSolBalance.toNumber(),
                "Staked balance should increase",
            );
            assert.equal(
                validatorAfter.totalDelegated.toNumber(),
                stakeAmount,
                "Validator should have correct delegation amount",
            );

            console.log("✅ All accounting verified!");
            console.log("💰 Stake will be ACTIVE in next epoch (~2-3 days)");
            console.log(
                `🎯 Stake account TO LOG!: ${stakeAccount.publicKey.toString()}`,
            );
        });

        it("Should reject staking with insufficient liquidity", async function () {
            this.timeout(30000);

            console.log("❌ Testing insufficient liquidity rejection...");

            const poolState = await program.account.stakingPool.fetch(pool);
            const excessiveAmount = poolState.liquidReserve.toNumber() +
                (10 * LAMPORTS_PER_SOL);

            console.log(
                `💰 Available: ${
                    poolState.liquidReserve.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );
            console.log(`🚫 Trying: ${excessiveAmount / LAMPORTS_PER_SOL} SOL`);

            const stakeConfigAccount = new PublicKey(
                "StakeConfig11111111111111111111111111111111",
            );
            const clock = await program.provider.connection.getSlot();

            try {
                await program.methods
                    .stakeToValidator(new anchor.BN(excessiveAmount), new anchor.BN(clock),)
                    .accounts({
                        authority: authority.publicKey,
                        validatorInfo: validatorInfo,
                        validatorVoteAccount: realValidatorVote,
                        stakeHistory: SYSVAR_STAKE_HISTORY_PUBKEY,
                        stakeConfig: stakeConfigAccount,
                    })
                    .signers([authority])
                    .rpc();

                assert.fail("Should have failed with insufficient liquidity");
            } catch (err) {
                assert.include(err.toString(), "InsufficientLiquidity");
                console.log("✅ Correctly rejected insufficient liquidity!");
            }
        });
    });

    describe("4. 🌾 Rewards Simulation", () => {
        it("Should simulate rewards update", async function () {
            this.timeout(30000);

            console.log("🌾 Simulating validator rewards...");

            const rewardsEarned = 0.1 * LAMPORTS_PER_SOL; // 0.1 SOL rewards
            const poolBefore = await program.account.stakingPool.fetch(pool);

            console.log(
                `💎 Exchange rate BEFORE: ${
                    poolBefore.exchangeRate.toNumber() / 1_000_000_000
                }`,
            );

            const tx = await program.methods
                .updateRewards(new anchor.BN(rewardsEarned))
                .accounts({
                    authority: authority.publicKey,
                })
                .signers([authority])
                .rpc();

            console.log(`✅ Rewards updated! TX: ${tx}`);

            const poolAfter = await program.account.stakingPool.fetch(pool);
            console.log(
                `💎 Exchange rate AFTER: ${
                    poolAfter.exchangeRate.toNumber() / 1_000_000_000
                }`,
            );
            console.log(
                `💰 Protocol fees earned: ${
                    poolAfter.protocolFeesEarned.toNumber() / LAMPORTS_PER_SOL
                } SOL`,
            );

            // Verify exchange rate increased
            assert.isTrue(
                poolAfter.exchangeRate.toNumber() >
                    poolBefore.exchangeRate.toNumber(),
                "Exchange rate should increase with rewards",
            );

            console.log("✅ FluidSOL tokens are now worth more SOL!");
        });
    });

    describe("5. 💸 Withdrawal Test", () => {
        it("Should withdraw SOL by burning FluidSOL", async function () {
            this.timeout(30000);

            console.log("💸 Testing FluidSOL withdrawal...");

            const withdrawAmount = 0.5 * LAMPORTS_PER_SOL; // 0.5 FluidSOL
            const userBalanceBefore = await provider.connection.getBalance(
                user.publicKey,
            );

            const tx = await program.methods
                .withdrawSol(new anchor.BN(withdrawAmount)) // instant withdrawal
                .accounts({
                    user: user.publicKey,
                    fluidSolMint: fluidSOLMint.publicKey,
                    userFluidSolAccount: userFluidSOLAccount,
                })
                .signers([user])
                .rpc();

            console.log(`✅ Withdrawal successful! TX: ${tx}`);

            const userBalanceAfter = await provider.connection.getBalance(
                user.publicKey,
            );
            const solReceived = (userBalanceAfter - userBalanceBefore) /
                LAMPORTS_PER_SOL;

            console.log(`💰 SOL received: ${solReceived} SOL`);
            console.log("✅ User successfully redeemed FluidSOL for SOL!");

            // Verify user received SOL
            assert.isTrue(
                userBalanceAfter > userBalanceBefore,
                "User should receive SOL",
            );
        });
    });

    after(() => {
        console.log("\n🎉 INTEGRATION TESTS COMPLETED!");
        console.log("🚀 Real validator staking successfully tested!");
        console.log("💡 Check Solana Explorer for transaction details");
    });
});
