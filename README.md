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
