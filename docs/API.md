# API Reference

Veiled exposes two gRPC services (Registry and Merchant), a set of REST API
routes via the Next.js web UI, and a wallet CLI binary.

---

## gRPC: Registry service (`proto/registry.proto`)

### Mutations

| RPC | Request | Response | Description |
|-----|---------|----------|-------------|
| `RegisterMerchant` | name, origin, email, phone | message | Register a merchant (rejects duplicates) |
| `CreateSet` | set_id, merchant_names, beneficiary_capacity, sats_per_user | message | Create anonymity set with CRS |
| `RegisterBeneficiary` | set_id, phi (33 bytes), name, email, phone, funding_txid, funding_vout | message, index | Pay fee + register commitment in set (verifies payment on-chain) |
| `FinalizeSet` | set_id, sats_per_user, funding_txid, funding_vout | message, root_txid, fanout_txid | Seal set, sign + broadcast VTxO tree |

### Queries

| RPC | Request | Response | Description |
|-----|---------|----------|-------------|
| `GetMerchants` | — | merchants[] (name, origin, credential_generator) | List all merchants |
| `GetCrs` | set_id | crs_bytes | Get serialized CRS for a set |
| `GetAnonymitySet` | set_id | commitments[], finalized, count, capacity | Get set status and commitments |
| `GetVtxoTree` | set_id | root_tx, fanout_tx | Get consensus-encoded Bitcoin transactions |
| `GetRegistryAddress` | set_id | address, internal_key | Get registry's P2TR payment address |
| `GetAggregateAddress` | set_id | address, aggregate_key | Get aggregate P2TR address for VTxO funding |
| `GetFees` | — | beneficiary_fee, merchant_fee | Get registration fee amounts (sats) |

### Streaming

| RPC | Request | Response | Description |
|-----|---------|----------|-------------|
| `SubscribeSetFinalization` | set_id | stream(commitments[], finalized, count, capacity) | Wait for set finalization |

---

## gRPC: Merchant service (`proto/merchant.proto`)

| RPC | Request | Response | Description |
|-----|---------|----------|-------------|
| `SubmitPaymentRegistration` | pseudonym, public_nullifier, set_id, service_index, friendly_name, proof | message | Register payment identity with ZK proof |
| `SubmitPaymentRequest` | amount, pseudonym, proof_r, proof_s | address, friendly_name | Beneficiary requests payment; merchant sends BTC to P2TR address |

---

## gRPC message details

### Registry messages

```protobuf
message MerchantRequest {
  string name = 1;
  string origin = 2;
  string email = 3;
  string phone = 4;
}

message CreateSetRequest {
  uint64 set_id = 1;
  repeated string merchant_names = 2;
  uint32 beneficiary_capacity = 3;
  uint64 sats_per_user = 4;
}

message BeneficiaryRequest {
  uint64 set_id = 1;
  bytes phi = 2;           // 33-byte commitment
  string name = 3;
  string email = 4;
  string phone = 5;
  bytes funding_txid = 6;  // 32-byte txid of fee payment
  uint32 funding_vout = 7; // output index within fee payment tx
}

message FinalizeSetRequest {
  uint64 set_id = 1;
  uint64 sats_per_user = 2;
  bytes funding_txid = 3;   // 32-byte txid
  uint32 funding_vout = 4;
}

message FinalizeSetResponse {
  string message = 1;
  string root_txid = 2;    // broadcast root transaction ID
  string fanout_txid = 3;  // broadcast fanout transaction ID
}

message GetRegistryAddressResponse {
  string address = 1;       // P2TR bech32m address (bcrt1p... on regtest)
  bytes internal_key = 2;   // 32-byte x-only public key
}

message GetAggregateAddressResponse {
  string address = 1;       // P2TR bech32m address for the aggregate key
  bytes aggregate_key = 2;  // 32-byte x-only aggregate public key
}

message GetFeesResponse {
  uint64 beneficiary_fee = 1;  // sats required per beneficiary registration
  uint64 merchant_fee = 2;     // sats required per merchant registration
}

message GetAnonymitySetResponse {
  repeated bytes commitments = 1;  // each 33 bytes
  bool finalized = 2;
  uint32 count = 3;
  uint32 capacity = 4;
}

message GetVtxoTreeResponse {
  bytes root_tx = 1;    // Bitcoin consensus-encoded transaction
  bytes fanout_tx = 2;  // Bitcoin consensus-encoded transaction
}
```

### Merchant messages

```protobuf
message PaymentRegistrationRequest {
  bytes pseudonym = 1;          // 33 bytes
  bytes public_nullifier = 2;   // 33 bytes
  uint64 set_id = 3;
  uint32 service_index = 4;
  string friendly_name = 5;
  bytes proof = 6;              // serialized PaymentIdentityRegistrationProof
}

message PaymentRequestMsg {
  uint64 amount = 1;
  bytes pseudonym = 2;    // 33 bytes
  bytes proof_r = 3;      // 33 bytes (nonce commitment)
  bytes proof_s = 4;      // 32 bytes (Schnorr response)
}

message PaymentRequestResponse {
  string address = 1;         // P2TR bitcoin address
  string friendly_name = 2;
}
```

---

## REST API routes (Next.js web UI)

These routes are served by the Next.js application and bridge the frontend
to gRPC services, Rust helper binaries, and the Bitcoin wallet.

### Setup

| Route | Method | Description |
|-------|--------|-------------|
| `/api/setup/init` | POST | Lazy set creation from registered merchants (idempotent) |
| `/api/state` | GET | Current simulation state |
| `/api/reset` | POST | Reset all state and kill spawned processes |

### Wallet

| Route | Method | Input | Description |
|-------|--------|-------|-------------|
| `/api/wallet/create` | POST | `{ name, role }` | Create BDK wallet for any participant |
| `/api/wallet/balance` | GET | `?name=X` | Sync wallet and return balance from bitcoind |
| `/api/wallet/send` | POST | `{ from, to_address, amount }` | Send BTC, returns txid |
| `/api/wallet/faucet` | POST | `{ names[] }` | Mine regtest blocks to fund listed wallets |
| `/api/wallet/tx` | GET | `?txid=X` | Get transaction details |

### Beneficiary

| Route | Method | Input | Description |
|-------|--------|-------|-------------|
| `/api/beneficiary/credential` | POST | `{ name }` | Create ZK credential (master identity) |
| `/api/beneficiary/register` | POST | `{ name }` | Pay fee + register Φ with the anonymity set |
| `/api/beneficiary/finalize` | POST | `{ name }` | Fund VTxO tree, finalize set, sign + broadcast txs |
| `/api/beneficiary/payment-id` | POST | `{ beneficiary, merchant }` | Register payment identity (ZK proof) |
| `/api/beneficiary/payment` | POST | `{ beneficiary, merchant, amount }` | Request payment from merchant (Schnorr proof) |
| `/api/beneficiary/merchants` | GET | — | List registered merchants from registry |
| `/api/beneficiary/incoming` | GET | `?name=X` | Check incoming payments received by beneficiary |

### Merchant

| Route | Method | Input | Description |
|-------|--------|-------|-------------|
| `/api/merchant/create` | POST | `{ name, origin }` | Spawn merchant gRPC server process |
| `/api/merchant/identities` | GET | `?name=X` | List registered beneficiaries at this merchant |
| `/api/merchant/payments` | GET | `?name=X` | Payment requests and sent payments |

---

## Wallet CLI (`veiled-wallet`)

Standalone binary for BIP86 P2TR wallet management. Built on
[`bdk_wallet`](https://crates.io/crates/bdk_wallet) v2 for descriptor-based
wallet operations and [`bdk_bitcoind_rpc`](https://crates.io/crates/bdk_bitcoind_rpc)
for chain synchronization. Communicates via JSON on stdin/stdout.

**Key design decisions:**

- **No bitcoind wallet needed** — BDK manages keys, descriptors, and address
  derivation entirely locally. The binary never calls `createwallet` or
  `loadwallet` on bitcoind, eliminating "database already exists" errors.
- **Ephemeral wallet recreation** — Each invocation recreates the BDK wallet
  from the stored mnemonic and descriptors. No persistent BDK database; the
  wallet is rebuilt from the JSON state file on every call.
- **Emitter-based sync** — Uses `bdk_bitcoind_rpc::Emitter` to walk the chain
  block-by-block and apply mempool updates, giving the wallet an accurate view
  of confirmed and unconfirmed balances.
- **Per-participant state** — Each participant (registry, merchant, beneficiary)
  has its own JSON state file in the `.wallets/` directory.

**State file format** (`<name>.json`):

```json
{
  "mnemonic": "twelve word mnemonic phrase ...",
  "descriptor": "tr([fingerprint/86'/1'/0']xprv.../0/*)",
  "change_descriptor": "tr([fingerprint/86'/1'/0']xprv.../1/*)",
  "address": "bcrt1p...",
  "address_index": 0,
  "network": "regtest"
}
```

Balances and UTXOs are always fetched live from bitcoind — never cached in state.

### Commands

#### `create-wallet`

Generate a new BIP39 mnemonic, derive BIP86 P2TR descriptors, create a BDK
wallet, and save the state file. If the state file already exists, the existing
wallet is loaded and returned (idempotent).

```json
// Input
{ "command": "create-wallet", "state_path": "/path/to/wallet.json", "name": "alice" }

// Output
{ "address": "bcrt1p...", "mnemonic": "word1 word2 ...", "wallet_name": "alice" }
```

Optional: `rpc_url`, `rpc_user`, `rpc_pass` (defaults: `http://localhost:18443`, `veiled`, `veiled`)

#### `get-balance`

Recreate the BDK wallet from state, sync with bitcoind via Emitter, and return
the current balance.

```json
// Input
{ "command": "get-balance", "state_path": "/path/to/wallet.json" }

// Output
{ "confirmed": 50000, "unconfirmed": 0, "total": 50000 }
```

#### `get-address`

Get a new receive address from the wallet.

```json
// Input
{ "command": "get-address", "state_path": "/path/to/wallet.json" }

// Output
{ "address": "bcrt1p..." }
```

#### `send`

Recreate the BDK wallet from state, sync UTXOs, build a PSBT, sign with BDK,
extract the final transaction, and broadcast via bitcoind RPC.

```json
// Input
{ "command": "send", "state_path": "/path/to/wallet.json", "to_address": "bcrt1p...", "amount_sats": 10000 }

// Output
{ "txid": "abc123..." }
```

#### `faucet`

Mine regtest blocks to a specified address (coinbase reward).

```json
// Input
{ "command": "faucet", "address": "bcrt1p...", "blocks": 1 }

// Output
{ "blocks_mined": 1, "block_hashes": ["0000..."] }
```

#### `get-tx`

Look up transaction details from bitcoind.

```json
// Input
{ "command": "get-tx", "txid": "abc123..." }

// Output
{ "txid": "abc123...", "confirmations": 6, "blockhash": "0000...", "size": 225, "vout": [...] }
```

#### `get-tx-history`

Recreate the BDK wallet from state, sync, and list all transactions using
`wallet.transactions()` and `wallet.sent_and_received()`. Classifies each
transaction as incoming or outgoing based on net sats flow.

```json
// Input
{ "command": "get-tx-history", "state_path": "/path/to/wallet.json" }

// Output
{ "transactions": [{ "txid": "...", "amount_sats": 5000, "direction": "incoming", "confirmations": 3 }] }
```

---

## Helper CLI (`veiled-helper`)

Rust binary that performs cryptographic operations for the web UI. Called
by Next.js API routes via `execFileSync` with JSON stdin/stdout.

| Command | Description |
|---------|-------------|
| `create-credential` | Generate master credential (Φ, sk, r, k) from CRS |
| `register-payment-id` | Derive pseudonym + nullifier, generate ZK proof |
| `create-payment-request` | Generate Schnorr proof for payment authentication |
