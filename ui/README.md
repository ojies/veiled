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
3. Next.js UI (port 3000)

Merchants are created dynamically through the UI — no pre-started merchants.

## Architecture

```
┌─────────────────────────────────────────┐
│           Next.js Web UI (:3000)        │
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

Every participant gets a BIP86 P2TR wallet backed by bitcoind:

- **Registry** — wallet created by `dev.sh` on startup, collects registration fees
- **Merchants** — wallet created when registering through the UI, pays 5,000 sats registration fee, sends payments to beneficiaries
- **Beneficiaries** — wallet auto-created with credential, pays 10,000 sats registration fee, receives payments from merchants

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
| `/api/beneficiary/register` | POST | Register with anonymity set |
| `/api/beneficiary/finalize` | POST | Finalize set with real UTXO |
| `/api/beneficiary/payment-id` | POST | Register payment identity (ZK proof) |
| `/api/beneficiary/payment` | POST | Request payment from merchant (Schnorr proof) |
| `/api/beneficiary/incoming` | GET | Check incoming payments to beneficiary |
| `/api/beneficiary/merchants` | GET | List registered merchants |
| `/api/merchant/identities` | GET | Registered beneficiaries at merchant |
| `/api/merchant/payments` | GET | Payment requests sent by merchant |
| `/api/state` | GET | Current simulation state |
| `/api/reset` | POST | Reset all state |

## Docker Deployment

The UI runs as a standalone Next.js container in Docker Compose. From the
project root:

```bash
docker compose up --build
```

The UI container bundles all Rust binaries (`veiled-helper`, `veiled-wallet`,
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
| `HELPER_BIN` | `../target/release/veiled-helper` | Path to helper binary |
| `MERCHANT_BIN` | `../target/release/merchant` | Path to merchant binary |
| `WALLETS_DIR` | `../.wallets` | Directory for wallet state files |
| `PROTO_DIR` | `../proto` | Directory containing .proto files |
