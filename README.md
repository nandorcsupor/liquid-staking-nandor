## Liquid Staking Protocol

## Blockchain

Anchor Framework - Solana smart contract development (Rust)
Solana Web3.js - Blockchain interaction from frontend
Target Network - Devnet → Mainnet

## Development Tools

Solana CLI - Blockchain deployment & management
Anchor CLI - Smart contract testing & deployment
Git - Version control (monorepo structure)

## 🚀 Development Environment Setup

Prerequisites
bash# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Get devnet SOL for testing

solana airdrop 5

## DEVNET to Mainnet

- Anchor.toml --> provider.cluster --> devnet/localnet
- solana config set --url devnet
- wallet = "~/.config/solana/devnet-keypair.json"

# NEW

- Delete target/deploy/keypair.json
- anchor build & keys sync 2-3x

# Anchor build

- `anchor build`
- `anchor build -p program_name`

# USER PUBLIC KEY:

- EW2VoijFGNg9B1xQHRyqHNCnqr4KNDLQrwECGH4NfswX

# background-service

- run: `cargo run`
- starts the backend which broadcasts prices of SOL/USDC from different sources for Arbitrage opportunities (or for fun)

  - 1. Raydium - Directly querying the program on the Solana blockchain, since Raydium does not use Anchor for their pools, there there was no available IDL. We use a crate for data parsing.
  - 2. Orca - through their SDK
  - 3. Meteora - chose DLMM = Bin-based liquidity. Liquidity is organized into price "bins". Trades only happen in the active bin.

  # Staking Pool Architecture

## 1. Global Pool Design 🏊‍♂️

### Pool Account Structure

seeds = [b"pool"] // Single global pool for all users

**Why Global Pool?**

- **Simplicity**: One pool serves all users
- **Program Redeployment Safe**: New program ID = new pool address automatically

### Pool Ownership

- **Pool Authority**: The account that initializes the pool becomes the owner
- **Management**: Only pool authority can add validators and manage protocol settings

## 2. Multiple Stake Accounts Per User 🎯

- seeds = `[b"stake", authority.key().as_ref(), &Clock::get()?.slot.to_le_bytes()]`

- Why Multiple Stake Accounts?
- **Solana Limitation: Cannot add SOL to existing stake accounts**
- **Multiple Staking: Users can stake multiple times with separat stake accounts accounts**
- **Automatic Uniqueness: Clock slot ensures each stake account is unique**

**FluidSOL Liquid Staking - Function Documentation**

**_Core Functions_**

✅ `initialize_pool` - Creates the main staking pool PDA using "pool" seed. Sets initial state: authority, exchange rate (1:1), reserves, fee structure (10% protocol fee), and validator count to 0.

✅ `add_validator` - Adds a validator to the pool's delegation strategy. Creates ValidatorInfo PDA with validator vote account, allocation percentage, and performance tracking. Only authority can add validators (max 10).

✅ `deposit_sol` - Users deposit SOL and receive FluidSOL tokens at current exchange rate. Transfers SOL to pool, mints FluidSOL tokens using pool PDA as mint authority, updates pool balances and adds to liquid reserve.

✅ `withdraw_sol` - Burns FluidSOL tokens and returns SOL. Supports instant withdrawal (0.3% fee) from liquid reserve or delayed withdrawal. Updates exchange rate accounting and pool state.

✅ `stake_to_validator` - Stakes SOL from liquid reserve to real validators. Creates stake account PDA, initializes it, transfers lamports, and delegates to validator vote account using CPI to stake program. Updates pool accounting.

✅ `harvest_rewards` - Checks stake account balance vs original delegation to detect rewards. Calculates protocol fee (10%), updates exchange rate to reflect increased SOL backing, and updates validator tracking.

✅ `update_rewards` - Manual rewards update function. Takes total rewards earned, splits into protocol fee (10%) and user rewards (90%), updates exchange rate to increase FluidSOL value.

✅ `rebalance_pool` - Maintains target reserve ratio (30%). Calculates if more SOL should be staked or unstaked to maintain optimal liquidity for instant withdrawals.

✅ `withdraw_protocol_fees` - Authority-only function to withdraw accumulated protocol fees from the pool. Transfers lamports directly from pool to authority account.

**Future Additions (road to production)**
Future Additions (road to production)
🔥 Critical Security Enhancements

1. Slashing Protection & Recovery

- Implement real-time slashing detection across all delegated validators
- Add adjust_for_slashing() function to handle validator penalties
- Exchange rate protection: prevent rate manipulation during slashing events
- Emergency pause mechanism for catastrophic slashing scenarios

2. Advanced Validator Management

- Performance-based rebalancing: Automatic stake redistribution based on validator APY and uptime
- Blacklist mechanism: Remove underperforming or malicious validators from pool
- Dynamic allocation: Adjust validator percentages based on real-time performance metrics
- Commission rate monitoring: Track and respond to validator fee changes

3. Economic Attack Prevention

- MEV protection: Implement slippage limits and sandwich attack prevention
- Flash loan protection: Minimum holding periods for large deposits/withdrawals
- Exchange rate bounds: Maximum daily exchange rate change limits (e.g., ±5%)
- Circuit breakers: Auto-pause during unusual market conditions

4. Production Infrastructure

- Multi-signature authority: Replace single authority with 3-of-5 multisig
- Automated reward harvesting: Background service to collect validator rewards
- Real unstaking implementation: 2-3 day withdrawal queue with proper epoch handling
- Comprehensive monitoring: Alerts for validator performance, pool health, and anomalies

📊 Additional Features

- Governance token: Community voting on validator selection and protocol parameters
- Insurance fund: Reserve pool to cover potential slashing losses
- Analytics dashboard: Real-time pool metrics, APY tracking, and validator performance
- Cross-program integration: Compatibility with DeFi protocols for yield farming
