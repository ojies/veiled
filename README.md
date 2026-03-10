# Veiled

An anonymous credential registry in Rust, inspired by the
[Anonymous Self-Credentials (ASC)](https://eprint.iacr.org/2025/anonymous-credentials) paper.

Users register a commitment to their identity on a public registry without
revealing who they are.  Service providers can verify that a user is a unique,
legitimate member of an anonymity set — without being able to link that user
across different services.

---

## Core idea

```
nullifier  = SHA256(pub_key || name)
commitment = SHA256(nullifier || blinding_key)
```

> **Why SHA256?** Consistency with the Bitcoin ecosystem — SHA256 is the
> universal standard there, has the widest tooling support, and is familiar
> to anyone working with Bitcoin primitives.

| Term | Meaning |
|---|---|
| **Public key** | 32-byte identity key owned by the user |
| **Name** | Human-readable handle (username) |
| **Nullifier** | Deterministic tag tying a key to a name — revealed on registration to prevent duplicate sign-ups (Sybil resistance) |
| **Commitment** | Hiding, binding commitment to the nullifier — stored in the anonymity set so the registry cannot link a commitment back to a specific identity |
| **Anonymity set** | A fixed-size batch of commitments; used later to generate zero-knowledge membership proofs |

---

## Architectural flow

```
┌─────────────────────────────────────────────────────────────────┐
│  User (veiled-cli)                                              │
│                                                                 │
│  1. Generate keys                                               │
│     pub_key  ← random 32 bytes (identity)                      │
│     blinding ← random 32 bytes (hiding factor)                 │
│                                                                 │
│  2. Derive credentials (LOCAL — never sent to server)           │
│     nullifier  = SHA256(pub_key ‖ name)                        │
│     commitment = SHA256(nullifier ‖ blinding)                  │
│                                                                 │
│  3. POST /api/v1/register  { commitment, nullifier }           │
└────────────────────────┬────────────────────────────────────────┘
                         │  HTTP
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│  Registry (veiled-registry)                                     │
│                                                                 │
│  register handler                                               │
│  ├── Reject if nullifier ∈ nullifier_index  → 409 Conflict     │
│  ├── Append commitment to current AnonymitySet                  │
│  │   └── If set is full → seal it, open a new one              │
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

When an anonymity set reaches its capacity (default **8**, configurable), it is
sealed.  New registrations go into a fresh set.  Sealed sets are immutable and
will serve as the accumulator for future ZK membership proofs.

---

## Project layout

```
veiled/
├── Cargo.toml                        # workspace
└── crates/
    ├── veiled-core/                  # cryptographic primitives & shared types
    │   └── src/
    │       ├── nullifier.rs          # compute_nullifier(pub_key, name) → SHA256(key||name)
    │       ├── commitment.rs         # commit(nullifier, blinding) → SHA256(nul||blind)
    │       └── types.rs              # PublicKey, Nullifier, Commitment, AnonymitySet
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
            └── main.rs               # generate-key, derive, register, has, sets, set
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
# commitment: <64 hex chars>

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
# 0      4      8          false

# 6. Inspect a specific set (shows all commitments)
veiled set --id 0

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
{ "commitment": "<64 hex>", "nullifier": "<64 hex>" }
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
[{ "id": 0, "size": 5, "capacity": 8, "full": false }]
```

---

### `GET /api/v1/sets/:id`

Return a full anonymity set with all commitment hex strings.

**Response**
```json
{ "id": 0, "commitments": ["aabb..."], "capacity": 8, "full": false }
```

---

## Testing

```bash
cargo test                     # all 24 tests across the workspace
cargo test -p veiled-core      # 9 crypto primitive unit tests
cargo test -p veiled-registry  # 6 unit tests + 9 HTTP integration tests
```

Test coverage:
- **veiled-core**: nullifier/commitment determinism, known SHA256 vectors, length checks
- **veiled-registry (unit)**: store registration, duplicate rejection, set rollover, DB persistence round-trips
- **veiled-registry (integration)**: all 4 HTTP endpoints exercised via `axum::Router::oneshot` (no network socket)

---

## Roadmap

- [x] SHA256-based nullifiers and commitments
- [x] SQLite persistence (write-through, survives restarts)
- [x] CLI client (generate-key, derive, register, has, sets, set)
- [x] Integration tests (HTTP-level, in-memory SQLite)
- [ ] Zero-knowledge membership proofs (ZK-SNARK / Bulletproofs)
- [ ] `POST /api/v1/verify` — verify a ZK proof of anonymity-set membership
- [ ] Pedersen commitment scheme (elliptic-curve based)
- [ ] CLI: load/save key material from a local keyfile
