# Veiled

An anonymous credential registry in Rust, inspired by the
[Anonymous Self-Credentials (ASC)](https://eprint.iacr.org/2025/618.pdf) paper.

Users register a commitment to their identity on a public registry without
revealing who they are.  Service providers can verify that a user is a unique,
legitimate member of an anonymity set — without being able to link that user
across different services.

See [SCENARIO.md](SCENARIO.md) for a concrete end-to-end walkthrough (Bob
verifies Alice's identity before sending her money).
See [ASC_COMPARISON.md](ASC_COMPARISON.md) for a detailed comparison between
veiled's implementation and the ASC paper protocol.

---

## Core idea

```
nullifier  = SHA256(pub_key || name)           # deterministic per (identity, name)
commitment = r·G + v·H  on secp256k1          # Pedersen commitment, v = scalar(nullifier)
```

> **Why secp256k1 Pedersen commitments?**  The scheme matches the
> [crypto-dbpoe](https://github.com/BoquilaID/U2SSO/tree/main/crypto-dbpoe)
> reference from the ASC paper.  Pedersen commitments are **homomorphic** —
> a property required for the Bootle/Groth membership proof — and secp256k1
> is the native curve of Bitcoin.

| Term | Meaning |
|---|---|
| **Public key** | 32-byte identity key owned by the user |
| **Name** | Human-readable handle (username) |
| **Nullifier** | `SHA256(pub_key ‖ name)` — deterministic per (identity, name); revealed on registration for Sybil resistance |
| **Commitment** | `r·G + v·H` — 33-byte compressed secp256k1 point; hiding and binding; stored in the anonymity set |
| **Blinding key** | Random 32-byte scalar `r`; keeps the nullifier hidden inside the commitment |
| **Anonymity set** | Fixed-size batch of 1024 commitments; sealed sets are the accumulator for ZK membership proofs |
| **Membership proof** | Bootle/Groth one-out-of-many proof — proves knowledge of one opening in the set without revealing which |

---

## Cryptographic primitives

### Nullifier — `SHA256(pub_key ‖ name)`

The nullifier is a **deterministic**, **one-way** tag.  It is the only value
sent to the registry alongside the commitment; this enables Sybil resistance
(the server rejects a second registration for the same nullifier) without
revealing the user's key or name.

### Pedersen commitment — `C = r·G + v·H`

- **G** — standard secp256k1 generator
- **H** — NUMS point: `hash_to_curve("veiled-H", dst="veiled-commitment-v1")`
- **v** = `scalar(nullifier.bytes)` — the committed value
- **r** = `scalar(blinding.bytes)` — the hiding factor

Properties:
- **Hiding**: given only `C`, an adversary cannot determine `v` without knowing `r`
- **Binding**: computationally infeasible to open `C` to a different `(v', r')` pair
- **Homomorphic**: `C(v₁,r₁) + C(v₂,r₂) = C(v₁+v₂, r₁+r₂)`

### One-out-of-many proof — Bootle/Groth 2015

Proves in zero knowledge that the prover knows an index `l` and opening
`(v, r)` such that `set[l] = r·G + v·H`, without revealing `l`.

Parameters: **M = 10**, **N = 2^M = 1024** (ring size matches the anonymity set capacity).

Proof size: **878 bytes**.

---

## Architectural flow

```
┌─────────────────────────────────────────────────────────────────┐
│  User (veiled-cli)                                              │
│                                                                 │
│  1. Generate keys                                               │
│     pub_key  ← random 32 bytes (identity)                      │
│     blinding ← random 32 bytes (hiding factor r)               │
│                                                                 │
│  2. Derive credentials (LOCAL — never sent to server)           │
│     nullifier  = SHA256(pub_key ‖ name)                        │
│     commitment = scalar(nullifier)·H + scalar(blinding)·G      │
│                                                                 │
│  3. POST /api/v1/register  { commitment, nullifier }           │
│                                                                 │
│  4. prove (LOCAL — once the anonymity set is sealed)            │
│     proof ← prove_membership(set, index, nullifier, blinding)  │
└────────────────────────┬────────────────────────────────────────┘
                         │  HTTP
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│  Registry (veiled-registry)                                     │
│                                                                 │
│  register handler                                               │
│  ├── Reject if nullifier ∈ nullifier_index  → 409 Conflict     │
│  ├── Append commitment to current AnonymitySet                  │
│  │   └── If set is full (1024) → seal it, open a new one       │
│  ├── Insert nullifier into nullifier_index  (O(1) HashSet)     │
│  └── Write-through to SQLite (commitments + nullifiers tables)  │
│                                                                 │
│  has handler  (POST /api/v1/has)                                │
│  ├── Recompute nullifier = SHA256(pub_key ‖ name)              │
│  └── Return { present: bool, nullifier }                        │
│                                                                 │
│  State layout                                                   │
│  ┌─────────────────────────┐   ┌──────────────────────────┐    │
│  │  RegistryStore (RAM)    │   │  veiled.db (SQLite)      │    │
│  │  sets: Vec<AnonymitySet>│◄──│  anonymity_sets table    │    │
│  │  nullifiers: HashSet    │◄──│  commitments table       │    │
│  └─────────────────────────┘   │  nullifiers table        │    │
│    live source of truth         └──────────────────────────┘    │
│    (loaded from SQLite on boot, written on every mutation)      │
└─────────────────────────────────────────────────────────────────┘
```

### Registration flow (step by step)

```
Client                          Registry                     SQLite
  │                                │                            │
  │── POST /register ─────────────►│                            │
  │   { commitment, nullifier }    │                            │
  │                                ├─ nullifier ∈ index? ──────►│
  │                                │  No → continue             │
  │                                ├─ push(commitment, set)     │
  │                                ├─ index.insert(nullifier)   │
  │                                ├── INSERT commitments ──────►│
  │                                ├── INSERT nullifiers ───────►│
  │◄── 200 { set_id, index } ──────│                            │
```

### Sybil resistance

The nullifier is a deterministic function of `(pub_key, name)`.  A user cannot
register the same `(pub_key, name)` pair twice — the server rejects the second
attempt with **409 Conflict**.  Because the nullifier is a one-way hash, the
server learns nothing about the user's public key or name from the commitment
alone.

### Anonymity set sealing

When an anonymity set reaches its capacity (**1024** commitments), it is
sealed.  New registrations go into a fresh set.  Sealed sets are immutable and
serve as the accumulator for the Bootle/Groth membership proof.

---

## Project layout

```
veiled/
├── Cargo.toml                        # workspace
├── examples/                         # workspace-level example assets
└── crates/
    ├── veiled-core/                  # cryptographic primitives & shared types
    │   ├── examples/
    │   │   ├── credentials.rs        # key generation + credential derivation
    │   │   ├── pedersen.rs           # Pedersen commitment properties
    │   │   └── membership_proof.rs   # full prove/verify over a 1024-element set
    │   └── src/
    │       ├── lib.rs                # public API re-exports
    │       ├── nullifier.rs          # compute_nullifier(pub_key, name) → SHA256(key‖name)
    │       ├── commitment.rs         # commit(nullifier, blinding) → r·G + v·H
    │       ├── proof.rs              # prove_membership / verify_membership (Bootle/Groth)
    │       └── types.rs              # PublicKey, Nullifier, Commitment, BlindingKey, AnonymitySet
    ├── veiled-registry/              # HTTP registry server with SQLite persistence
    │   ├── src/
    │   │   ├── lib.rs                # public module re-exports (used by integration tests)
    │   │   ├── db.rs                 # SQLite read/write (rusqlite, write-through)
    │   │   ├── store.rs              # in-memory state (anonymity sets + nullifier index)
    │   │   ├── error.rs              # AppError → HTTP responses
    │   │   ├── server.rs             # axum router + AppState
    │   │   ├── main.rs               # entry point
    │   │   └── routes/
    │   │       ├── register.rs       # POST /api/v1/register
    │   │       ├── has.rs            # POST /api/v1/has
    │   │       └── sets.rs           # GET  /api/v1/sets[/:id]
    │   └── tests/
    │       └── api.rs                # HTTP integration tests (no server socket needed)
    └── veiled-cli/                   # Command-line client
        └── src/
            └── main.rs               # generate-key, derive, register, has, sets, set, prove
```

---

## Running the registry server

```bash
cargo run -p veiled-registry
# INFO veiled_registry: database: veiled.db
# INFO veiled_registry: loaded 1 set(s), 0 nullifier(s)
# INFO veiled_registry: veiled registry listening on 0.0.0.0:7271
```

| Env var | Default | Description |
|---|---|---|
| `VEILED_DB` | `veiled.db` | SQLite database path |
| `VEILED_PORT` | `7271` | TCP port to listen on |
| `RUST_LOG` | — | Log verbosity (`info`, `debug`, …) |

State persists across restarts automatically.

---

## CLI client

All subcommands default to `http://localhost:7271`. Override with `--server <url>`.

```bash
# 1. Generate a fresh identity key + blinding key
veiled generate-key
# pub_key:  <64 hex chars>
# blinding: <64 hex chars>

# 2. Derive credentials locally — nothing sent to the server
veiled derive \
  --pub-key <64 hex chars> \
  --name alice \
  --blinding <64 hex chars>    # random if omitted
# nullifier:  <64 hex chars>
# commitment: <66 hex chars>   (33-byte compressed EC point)

# 3. Register your identity with the registry
veiled register \
  --pub-key <64 hex chars> \
  --name alice \
  --blinding <64 hex chars>    # must match the blinding used in derive
# registered → set_id=0, index=3

# 4. Check if an identity is registered
veiled has --pub-key <64 hex chars> --name alice
# present:  true
# nullifier: <64 hex chars>

# 5. List all anonymity sets
veiled sets
# id     size   capacity   full
# 0      4      1024       false

# 6. Inspect a specific set (shows all commitments)
veiled set --id 0

# 7. Generate a zero-knowledge membership proof
#    (fetches the set from the server, proves locally — ~2–5 s release build)
veiled prove \
  --pub-key  <64 hex chars> \
  --name     alice \
  --blinding <64 hex chars> \
  --set-id   0 \
  --index    3               # returned by `register`
# proof (878 bytes):
# <1756 hex chars>

# 8. Save key material to a JSON keyfile
veiled save-key \
  --pub-key  <64 hex chars> \
  --blinding <64 hex chars> \
  --out      veiled-keys.json
# keys saved to veiled-keys.json

# 9. Load key material from a keyfile
veiled load-key --file veiled-keys.json
# pub_key:  <64 hex chars>
# blinding: <64 hex chars>

# Override the server URL
veiled --server http://example.com:7271 sets
```

---

## REST API

### `POST /api/v1/register`

Register a commitment + nullifier pair.  Returns conflict if the nullifier
has already been used (Sybil resistance).

**Request**
```json
{ "commitment": "<66 hex>", "nullifier": "<64 hex>" }
```

**Response 200**
```json
{ "set_id": 0, "index": 3 }
```

**Response 409** — nullifier already registered
```json
{ "error": "nullifier already registered" }
```

---

### `POST /api/v1/has`

Check whether a `(pub_key, name)` pair is registered. The server recomputes
`SHA256(pub_key || name)` and looks it up in its nullifier index.

**Request**
```json
{ "pub_key": "<64 hex>", "name": "alice" }
```

**Response**
```json
{ "present": true, "nullifier": "<64 hex>" }
```

---

### `GET /api/v1/sets`

List all anonymity sets (summary).

**Response**
```json
[{ "id": 0, "size": 5, "capacity": 1024, "full": false }]
```

---

### `GET /api/v1/sets/:id`

Return a full anonymity set with all commitment hex strings.

**Response**
```json
{ "id": 0, "commitments": ["02aabb..."], "capacity": 1024, "full": false }
```

---

### `POST /api/v1/verify`

Verify a 878-byte one-out-of-many membership proof server-side.

**Request**
```json
{ "nullifier": "<64 hex>", "set_id": 0, "proof": "<1756 hex>" }
```

**Response 200**
```json
{ "valid": true }
```

**Response 400** — bad hex or wrong proof length

**Response 404** — set_id not found

---

## Examples

```bash
# Credential derivation and property checks (instant)
cargo run --example credentials -p veiled-core

# Pedersen commitment properties including homomorphic addition (instant)
cargo run --example pedersen -p veiled-core

# Full 1024-element anonymity set: prove + verify (~2–5 s release, ~90 s debug)
cargo run --example membership_proof -p veiled-core --release
```

---

## Testing

```bash
cargo test                     # all tests across the workspace
cargo test -p veiled-core      # 14 crypto primitive unit tests
cargo test -p veiled-registry  # 6 unit tests + 9 HTTP integration tests
```

Test coverage:
- **veiled-core**: nullifier determinism; Pedersen commitment properties (determinism, hiding, binding, SEC1 encoding); one-out-of-many proof correctness, tamper detection, wrong-nullifier rejection
- **veiled-registry (unit)**: store registration, duplicate rejection, set rollover, DB persistence round-trips
- **veiled-registry (integration)**: all 4 HTTP endpoints exercised via `axum::Router::oneshot` (no network socket)

---

## Roadmap

- [x] SHA256-based nullifiers
- [x] Pedersen commitments on secp256k1 (`C = r·G + v·H`)
- [x] Bootle/Groth one-out-of-many membership proof (M=10, N=1024)
- [x] SQLite persistence (write-through, survives restarts)
- [x] CLI client (generate-key, derive, register, has, sets, set, prove, save-key, load-key)
- [x] Integration tests (HTTP-level, in-memory SQLite)
- [x] Usage examples (credentials, pedersen, membership_proof)
- [x] `POST /api/v1/verify` — server-side ZK proof verification endpoint
- [x] CLI: load/save key material from a local keyfile
- [ ] Pedersen commitment upgrade to use `hash_to_scalar` for the nullifier value
