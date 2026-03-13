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
- **Merchants** — wallet created when registering through the UI, pays 5,000 sats registration fee
- **Beneficiaries** — wallet auto-created with credential, pays 10,000 sats registration fee

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
| `/api/beneficiary/payment` | POST | Request payment (Schnorr proof) |
| `/api/beneficiary/incoming` | GET | Check incoming payments |
| `/api/beneficiary/merchants` | GET | List registered merchants |
| `/api/merchant/identities` | GET | Registered beneficiaries |
| `/api/merchant/payments` | GET | Payment history |
| `/api/state` | GET | Current simulation state |
| `/api/reset` | POST | Reset all state |

## Development

```bash
cd ui
npm install
npm run dev
```

The UI expects:
- Registry at `[::1]:50051`
- `bitcoind` at `localhost:18443` (user: `veiled`, pass: `veiled`)
- Rust binaries built in `../target/release/`

Environment variables:
- `BITCOIN_RPC_URL` (default: `http://localhost:18443`)
- `BITCOIN_RPC_USER` (default: `veiled`)
- `BITCOIN_RPC_PASS` (default: `veiled`)
