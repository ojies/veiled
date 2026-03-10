# Veiled

An anonymous credential system on **Bitcoin**, implementing the
[Anonymous Self-Credentials (ASC)](annomymous-credential.pdf) protocol
with a Common Reference String (CRS) and Bootle/Groth one-out-of-many proofs.

Users generate a master credential locally and register a multi-value Pedersen
commitment on a public identity registry backed by Bitcoin vtxo-trees.
Service providers can verify that a user is a unique, legitimate member of an
anonymity set — without being able to link that user across different services.

See [SCENARIO.md](SCENARIO.md) for a concrete end-to-end walkthrough.
See [ASC_COMPARISON.md](ASC_COMPARISON.md) for a detailed comparison between
veiled's implementation and the ASC paper protocol.

---

## Protocol overview

The CRS-ASC protocol has three phases:

### Phase 0 — System Setup

A trusted setup generates the Common Reference String (CRS):

```
crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)
```

- **G** = secp256k1, **q** = curve order
- **g** = HashToCurve("CRS-ASC-generator-0") — base generator (NUMS)
- **h_l** = HashToCurve("CRS-ASC-generator-{l}") for l = 1..L — per-provider generators
- **v_l** = service provider name (string identifier, used as HKDF salt)
- **G_auth_l** = credential generator for provider l
- Identity Registry (IdR) initialized with N = 1024

All generators are derived via hash-to-curve with DST `"CRS-ASC-v1"`, ensuring
they are provably independent (NUMS — Nothing Up My Sleeve).

### Phase 1 — Master Credential Creation (local, offline)

The user generates three secrets locally:

```
sk ←$ {0,1}^256     # root secret for nullifier derivation (MasterSecret)
r  ←$ {0,1}^256     # child credential randomness (ChildRandomness)
k  ←$ Z_q           # Pedersen blinding key (BlindingKey)
```

Then derives L nullifier scalars and computes the master identity:

```
for l = 1..L:
  s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")  # per-service nullifier scalar
  nul_l = s_l · g                                       # public nullifier (group element)

Φ = k·g + s_1·h_1 + ... + s_L·h_L                      # multi-value Pedersen commitment

Master credential = (Φ, sk, r, k)                       # user stores ~96 bytes (sk, r, k)
```

### Phase 2 — Master Identity Registration (on-chain via Bitcoin)

```
send Φ to IdR (Bitcoin vtxo-tree, not Ethereum)
wait for Λ_{d̂} to fill to N = 1024
receive Λ_{d̂} = [Φ_1, ..., Φ_1024]
determine own index j
store (Φ_j, sk, r, k, d̂, Λ_{d̂})
```

The sealed anonymity set is anchored on Bitcoin via a vtxo-tree: each commitment
(a 33-byte compressed secp256k1 point) becomes a P2TR leaf key directly.

---

## Core cryptographic primitives

### Multi-value Pedersen commitment

```
Φ = k·g + s_1·h_1 + ... + s_L·h_L
```

Where `g, h_1..h_L` are L+1 independent generators from the CRS.
The commitment hides L nullifier values under a single blinding key k.

Properties:
- **Hiding**: given only Φ, an adversary cannot determine any s_l without k
- **Binding**: computationally infeasible to open Φ to different values
- **Homomorphic**: required for the Bootle/Groth membership proof

### HKDF per-verifier nullifier derivation

```
s_l = HKDF-SHA256(IKM = sk, salt = v_l, info = "CRS-ASC-nullifier")
```

Each master secret produces L different nullifiers (one per service provider).
Same master secret + different service → different unlinkable nullifiers.
This gives **automatic cross-service unlinkability** — a protocol property,
not a manual workaround.

### Public nullifier (group element)

```
nul_l = s_l · g
```

Serves double duty:
- **Sybil-resistance token**: unique per (master identity, service provider)
- **Public authentication key**: the user can prove knowledge of s_l

### One-out-of-many proof — Bootle/Groth 2015

Proves in zero knowledge that the prover knows an index `l` and opening
such that `set[l]` is their commitment, without revealing `l`.

Parameters: **M = 10**, **N = 2^M = 1024** (ring size matches anonymity set capacity).

Proof size: **878 bytes**.

---

## Terminology

| Term | Meaning |
|---|---|
| **CRS** | Common Reference String — public parameters `(g, h_1..h_L, v_1..v_L)` |
| **Master secret (sk)** | 32-byte root secret for HKDF nullifier derivation |
| **Child randomness (r)** | 32-byte randomness for service-specific auth key derivation |
| **Blinding key (k)** | 32-byte Pedersen blinding scalar |
| **Nullifier scalar (s_l)** | `HKDF(sk, v_l)` — raw 32-byte scalar, one per service provider |
| **Public nullifier (nul_l)** | `s_l · g` — 33-byte compressed secp256k1 point |
| **Master identity (Φ)** | `k·g + Σ s_l·h_l` — 33-byte multi-value Pedersen commitment |
| **Master credential** | Tuple `(Φ, sk, r, k)` — user stores locally |
| **Registered identity** | `(Φ_j, sk, r, k, d̂, Λ_{d̂})` — includes frozen anonymity set and own index |
| **Anonymity set (Λ)** | Fixed-size batch of 1024 commitments; sealed sets are the accumulator for ZK proofs |
| **vtxo-tree** | Binary pre-signed transaction tree anchoring commitments on Bitcoin |

---

## Architectural flow

```
┌──────────────────────────────────────────────────────────────────┐
│  Phase 0: Trusted Setup                                          │
│                                                                  │
│  CRS = (g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)            │
│  g = HashToCurve("CRS-ASC-generator-0", DST="CRS-ASC-v1")      │
│  h_l = HashToCurve("CRS-ASC-generator-{l}", DST="CRS-ASC-v1")  │
│  IdR initialized with N = 1024                                   │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│  User (Phase 1 — local, offline)                                 │
│                                                                  │
│  1. Generate secrets: sk, r, k ←$ random                         │
│  2. Derive nullifiers: s_l = HKDF(sk, v_l) for l = 1..L         │
│  3. Compute Φ = k·g + s_1·h_1 + ... + s_L·h_L                  │
│  4. Store (sk, r, k) securely (~96 bytes)                        │
│                                                                  │
│  5. POST /api/v1/register-identity { commitment: Φ, nullifiers } │
│                                                                  │
│  6. Wait for anonymity set Λ_{d̂} to fill to N=1024             │
│  7. Determine own index j in Λ_{d̂}                              │
│  8. Store (Φ_j, sk, r, k, d̂, Λ_{d̂})                           │
└────────────────────────┬─────────────────────────────────────────┘
                         │  HTTP
                         ▼
┌──────────────────────────────────────────────────────────────────┐
│  Registry (veiled-registry)                                      │
│                                                                  │
│  register-identity handler                                       │
│  ├── Check ALL L nullifiers for duplicates (atomic)              │
│  ├── Append Φ to current AnonymitySet                            │
│  │   └── If set is full (1024) → seal it, open a new one        │
│  ├── Insert all nullifiers into index                            │
│  └── Write-through to SQLite                                     │
│                                                                  │
│  On set seal: anchor on Bitcoin via vtxo-tree                    │
│  ├── Each Φ (33-byte secp256k1 point) → P2TR leaf key           │
│  └── Pre-signed binary transaction tree for 1024 leaves          │
│                                                                  │
│  State layout                                                    │
│  ┌─────────────────────────┐   ┌──────────────────────────┐     │
│  │  RegistryStore (RAM)    │   │  veiled.db (SQLite)      │     │
│  │  sets: Vec<AnonymitySet>│◄──│  anonymity_sets table    │     │
│  │  nullifiers: HashSet    │◄──│  commitments table       │     │
│  └─────────────────────────┘   │  nullifiers table        │     │
│    live source of truth         └──────────────────────────┘     │
│    (loaded from SQLite on boot, written on every mutation)       │
└──────────────────────────────────────────────────────────────────┘
```

### Sybil resistance

Each master identity produces L nullifiers — one per service provider — via
`s_l = HKDF(sk, v_l)`. At registration time, ALL L nullifiers are submitted
and checked atomically. A user cannot register the same master secret twice
(any duplicate nullifier triggers rejection with **409 Conflict**).

Because nullifiers are derived via HKDF with the service name as salt,
different services see different nullifiers from the same user — providing
**automatic cross-service unlinkability** as a protocol property.

### Anonymity set sealing

When an anonymity set reaches its capacity (**1024** commitments), it is
sealed. New registrations go into a fresh set. Sealed sets are immutable and
serve as the accumulator for the Bootle/Groth membership proof.

Sealed sets are anchored on Bitcoin via vtxo-trees: each commitment is a valid
secp256k1 point and becomes a P2TR leaf key in the transaction tree.

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
    │       ├── crs.rs                # CRS setup, multi-value Pedersen commitment, HashToCurve generators
    │       ├── credential.rs         # MasterCredential (Phase 1) + RegisteredIdentity (Phase 2)
    │       ├── nullifier_v2.rs       # HKDF per-verifier nullifier derivation + public nullifiers
    │       ├── nullifier.rs          # legacy SHA256(pub_key || name) nullifier (backward compat)
    │       ├── commitment.rs         # single-value Pedersen commit (used by Bootle/Groth proof)
    │       ├── proof.rs              # prove_membership / verify_membership (Bootle/Groth)
    │       └── types.rs              # MasterSecret, ChildRandomness, BlindingKey, Nullifier, Commitment, Name, ...
    ├── veiled-registry/              # HTTP registry server with SQLite persistence
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── main.rs               # entry point
    │   │   ├── server.rs             # axum router + AppState
    │   │   ├── db.rs                 # SQLite read/write (write-through)
    │   │   ├── store.rs              # in-memory state (anonymity sets + nullifier index)
    │   │   ├── error.rs              # AppError → HTTP responses
    │   │   ├── bitcoin_anchor.rs     # vtxo-tree anchoring for sealed anonymity sets
    │   │   └── routes/
    │   │       ├── register.rs       # POST /api/v1/register + POST /api/v1/register-identity
    │   │       ├── has.rs            # POST /api/v1/has
    │   │       ├── sets.rs           # GET  /api/v1/sets[/:id]
    │   │       └── verify.rs         # POST /api/v1/verify
    │   └── tests/
    │       └── api.rs                # HTTP integration tests
    ├── veiled-cli/                   # Command-line client
    │   └── src/
    │       └── main.rs
    └── vtxo-tree/                    # Bitcoin vtxo-tree (pre-signed tx tree for 1024 users)
        ├── src/
        │   ├── lib.rs
        │   ├── tree.rs
        │   ├── types.rs
        │   └── tx.rs
        └── tests/
            ├── integration.rs
            └── e2e.rs
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

## REST API

### `POST /api/v1/register-identity` (ASC protocol)

Register a master identity commitment with all L per-service-provider nullifiers.
Returns conflict if ANY nullifier has already been registered (atomic check).

**Request**
```json
{
  "commitment": "<66 hex chars>",
  "nullifiers": ["<64 hex chars>", ...]
}
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

### `POST /api/v1/register` (legacy, single nullifier)

Register a commitment + single nullifier pair. Kept for backward compatibility.

**Request**
```json
{ "commitment": "<66 hex>", "nullifier": "<64 hex>" }
```

**Response 200**
```json
{ "set_id": 0, "index": 3 }
```

---

### `POST /api/v1/has`

Check whether a `(pub_key, name)` pair is registered.

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
cargo test                      # all tests across the workspace
cargo test -p veiled-core       # 50 crypto primitive + protocol tests
cargo test -p veiled-registry   # 27 unit + integration tests
```

Test coverage:
- **veiled-core**: CRS generator independence and determinism; multi-value Pedersen commitment properties; HKDF nullifier derivation (determinism, uniqueness, cross-service independence); public nullifier validity; MasterCredential creation and recomputation; RegisteredIdentity index determination; full Phase 1+2 flow; Bootle/Groth proof correctness
- **veiled-registry (unit)**: store registration (single + multi-nullifier), duplicate rejection (atomic), set rollover, DB persistence round-trips, Bitcoin anchor (commitment-to-user, vtxo-tree construction, CRS-to-anchor flow)
- **veiled-registry (integration)**: all HTTP endpoints exercised via `axum::Router::oneshot` (no network socket)

---

## Roadmap

- [x] CRS generation with HashToCurve generators (Phase 0)
- [x] Multi-value Pedersen commitments `Φ = k·g + Σ s_l·h_l` (Phase 0)
- [x] HKDF per-verifier nullifier derivation with automatic cross-service unlinkability (Phase 0)
- [x] Public nullifiers `nul_l = s_l · g` as Sybil-resistance tokens (Phase 1)
- [x] MasterCredential creation with recomputable Φ (Phase 1)
- [x] RegisteredIdentity with index determination (Phase 2)
- [x] Multi-nullifier atomic registration endpoint (Phase 2)
- [x] Bitcoin anchoring via vtxo-tree (sealed sets → P2TR leaf keys) (Phase 2)
- [x] Bootle/Groth one-out-of-many membership proof (M=10, N=1024)
- [x] SQLite persistence (write-through, survives restarts)
- [x] CLI client (generate-key, derive, register, has, sets, set, prove, save-key, load-key)
- [x] Integration tests (HTTP-level, in-memory SQLite)
- [x] `POST /api/v1/verify` — server-side ZK proof verification endpoint
- [ ] Phase 3: Service-specific credential derivation + Bootle/Groth proof adaptation for multi-value commitments
- [ ] Phase 4: Anonymous authentication protocol
