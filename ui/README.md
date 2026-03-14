# Veiled Web UI

Interactive demo of the Veiled pseudonymous payment protocol on Bitcoin. Each participant (registry, merchants, beneficiaries) has a real Bitcoin wallet on regtest with live transactions.

## Prerequisites

- **Rust** (cargo)
- **Node.js** 18+ and npm
- **Bitcoin Core** (`bitcoind` and `bitcoin-cli` in PATH)

## Quick Start

From the project root:

```bash
./scripts/dev.sh
```

This starts:
1. `bitcoind` in regtest mode (port 18443)
2. Registry gRPC server (port 50051)
3. Web UI (port 3000)

Merchants are created dynamically through the UI — no pre-started merchants.

## Architecture

```
┌─────────────────────────────────────────┐
│              Web UI (:3000)             │
│  Landing │ Beneficiary │ Merchant       │
├─────────────────────────────────────────┤
│              API Routes                 │
│  /api/setup/init   /api/wallet/*        │
│  /api/beneficiary/* /api/merchant/*     │
├──────────┬──────────┬───────────────────┤
│  gRPC    │  veiled- │  veiled-          │
│  Client  │  helper  │  wallet           │
├──────────┼──────────┼───────────────────┤
│ Registry │ Crypto   │ Bitcoin           │
│ :50051   │ (stdin/  │ Wallet            │
│          │  stdout) │ (stdin/stdout)    │
├──────────┴──────────┼───────────────────┤
│                     │ bitcoind (regtest)│
│                     │ :18443            │
└─────────────────────┴───────────────────┘
```

## Wallet Flow

Every participant gets a BIP86 P2TR wallet managed locally by BDK (`bdk_wallet` v2).
Keys and descriptors are stored in per-participant JSON state files — no bitcoind
wallet creation is needed. Chain data (UTXOs, balances) is synced from bitcoind
via `bdk_bitcoind_rpc`'s `Emitter` pattern (block-by-block + mempool):

- **Registry** — wallet created by `dev.sh` on startup, collects registration fees
- **Merchants** — wallet created when registering through the UI, pays registration fee (default 3,000 sats), sends payments to beneficiaries
- **Beneficiaries** — wallet auto-created with credential, pays registration fee (default 2,000 sats), receives payments from merchants

Fee amounts are configured on the registry server and fetched by the UI via
the `GetFees` gRPC call — no fee-related environment variables are needed on
the UI side.

The **faucet** button mines regtest blocks to fund wallets instantly.

## API Routes

| Route | Method | Description |
|-------|--------|-------------|
| `/api/setup/init` | POST | Lazy set creation from registered merchants |
| `/api/merchant/create` | POST | Spawn merchant gRPC server |
| `/api/wallet/create` | POST | Create wallet for any participant |
| `/api/wallet/balance` | GET | Query wallet balance |
| `/api/wallet/send` | POST | Send BTC between wallets |
| `/api/wallet/faucet` | POST | Mine blocks to fund wallets |
| `/api/wallet/tx` | GET | Transaction details |
| `/api/beneficiary/credential` | POST | Create ZK credential |
| `/api/beneficiary/register` | POST | Pay fee + register with anonymity set |
| `/api/beneficiary/finalize` | POST | Fund Taproot commitment, finalize set |
| `/api/beneficiary/payment-id` | POST | Register payment identity (ZK proof) |
| `/api/beneficiary/payment` | POST | Request payment from merchant (Schnorr proof) |
| `/api/beneficiary/incoming` | GET | Check incoming payments to beneficiary |
| `/api/beneficiary/merchants` | GET | List registered merchants |
| `/api/merchant/identities` | GET | Registered beneficiaries at merchant |
| `/api/merchant/payments` | GET | Payment requests sent by merchant |
| `/api/state` | GET | Current simulation state |
| `/api/reset` | POST | Reset all state |

## Docker Deployment

The UI runs as a standalone container in Docker Compose. From the
project root:

```bash
docker compose up --build
```

The UI container bundles all Rust binaries (`veiled-core`, `veiled-wallet`,
`merchant`) and spawns them as child processes. Configuration is via
environment variables — see `docker-compose.yml` for the full list.

## Local Development

```bash
cd ui
npm install
npm run dev
```

The UI expects:
- Registry at `[::1]:50051`
- `bitcoind` at `localhost:18443` (user: `veiled`, pass: `veiled`)
- Rust binaries built in `../target/release/`

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BITCOIN_RPC_URL` | `http://localhost:18443` | bitcoind RPC endpoint |
| `BITCOIN_RPC_USER` | `veiled` | bitcoind RPC username |
| `BITCOIN_RPC_PASS` | `veiled` | bitcoind RPC password |
| `REGISTRY_ADDRESS` | `[::1]:50051` | Registry gRPC address |
| `REGISTRY_SERVER` | `http://[::1]:50051` | Registry gRPC URL (for merchant binary) |
| `WALLET_BIN` | `../target/release/veiled-wallet` | Path to wallet binary |
| `HELPER_BIN` | `../target/release/veiled-core` | Path to helper binary |
| `MERCHANT_BIN` | `../target/release/merchant` | Path to merchant binary |
| `WALLETS_DIR` | `../.wallets` | Directory for wallet state files |
| `PROTO_DIR` | `../proto` | Directory containing .proto files |
| `BENEFICIARY_CAPACITY` | `2` | Slots per anonymity set (must be power of 2) |
| `MIN_MERCHANTS` | `1` | Minimum merchants before set creation |
| `MERCHANT_START_PORT` | `50061` | Starting port for merchant gRPC servers |
| `MERCHANT_STARTUP_DELAY` | `1500` | Wait time (ms) after spawning merchant |
| `MATURITY_BLOCKS` | `100` | Blocks mined to mature coinbase outputs |
