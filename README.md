# Veiled

An anonymous credential system on **Bitcoin**, implementing the
[Anonymous Self-Credentials (ASC)](docs/annomymous-credential.pdf) protocol
with a Common Reference String (CRS) and Bootle/Groth one-out-of-many proofs.

Users generate a master credential locally and register a multi-value Pedersen
commitment on a public identity registry backed by Bitcoin vtxo-trees.
Service providers can verify that a user is a unique, legitimate member of an
anonymity set — without being able to link that user across different services.

See [SCENARIO.md](docs/SCENARIO.md) for a concrete end-to-end walkthrough.
See [ASC_COMPARISON.md](docs/ASC_COMPARISON.md) for a detailed comparison between
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
send Φ to IdR (Bitcoin, not Ethereum)
wait for Λ_{d̂} to fill to N = 1024
receive Λ_{d̂} = [Φ_1, ..., Φ_1024]
determine own index j
store (Φ_j, sk, r, k, d̂, Λ_{d̂})
```

Once 1024 users have registered and the anonymity set is full, it is **sealed**
(frozen — no more additions, no removals, ever). The sealed set is then anchored
on Bitcoin using a **vtxo-tree** (virtual transaction output tree):

- A vtxo-tree is a binary tree of **pre-signed Bitcoin transactions**. Only the
  root transaction is broadcast on-chain. The interior nodes are connector
  transactions, and the 1024 leaves are individual outputs — one per user.
- Each leaf output is a **P2TR (Pay-to-Taproot)** output whose internal key is
  the user's commitment `Φ`. This works because `Φ` is a 33-byte compressed
  secp256k1 point — which is already a valid public key.
- The single on-chain UTXO (the root) cryptographically commits to all 1024
  identities via the transaction tree structure. This is far more efficient than
  storing 1024 entries in an Ethereum smart contract.
- Spending any leaf requires proving knowledge of the commitment opening
  (the secret values `k, s_1..s_L`) — this is where the ZK proof comes in.

After sealing, the user downloads the complete set `Λ_{d̂} = [Φ_1, ..., Φ_1024]`
and finds their own position `j` in the list. They need the full set stored
locally because the Bootle/Groth zero-knowledge proof requires the prover to
have the entire ring of 1024 commitments — proving "I know the opening to one
of these 1024 commitments" means knowing all 1024 to construct the proof.

---

## Core cryptographic primitives

### Multi-value Pedersen commitment

```
Φ = k·g + s_1·h_1 + ... + s_L·h_L
```

Where `g, h_1..h_L` are L+1 independent generators from the CRS.
This packs L separate nullifier values into a single 33-byte elliptic curve
point, hidden by the blinding key k. Think of it as a sealed envelope
containing L secrets — anyone can see the envelope (Φ), but nobody can read
what's inside without knowing k.

Properties:
- **Hiding**: given only Φ, an adversary cannot determine any s_l without k
- **Binding**: computationally infeasible to find a different set of values
  (s_1', ..., s_L', k') that produce the same Φ. This means each user is
  locked to exactly one nullifier per service — Sybil resistance at the math level.
- **Homomorphic**: commitments can be added together, which is required for
  the Bootle/Groth membership proof to work over the set

### HKDF per-verifier nullifier derivation

```
s_l = HKDF-SHA256(IKM = sk, salt = v_l, info = "CRS-ASC-nullifier")
```

HKDF (HMAC-based Key Derivation Function, RFC 5869) is a standard way to
derive multiple independent keys from one master secret. Here, the same `sk`
is combined with different service names to produce different nullifier scalars:

- `HKDF(sk, "twitter.com")` → `s_1` (a 32-byte scalar for Twitter)
- `HKDF(sk, "github.com")` → `s_2` (a completely different 32-byte scalar for GitHub)

HKDF's security guarantee: even if you know both service names, the two outputs
are computationally indistinguishable from independent random values. This gives
**automatic cross-service unlinkability** — a protocol property, not a manual
workaround. Two colluding services cannot determine that their nullifiers came
from the same user.

### Public nullifier (group element)

```
nul_l = s_l · g     (scalar multiplication — produces a curve point)
```

The raw scalar `s_l` is a secret (it's inside the commitment). The public
nullifier `nul_l` is what gets shown to service providers — it's the result
of multiplying the secret scalar by the generator point g. This is a one-way
operation (you can't recover `s_l` from `nul_l` without solving the discrete
log problem).

Serves double duty:
- **Sybil-resistance token**: same `sk` + same service always produces the same
  `nul_l`, so a service can detect if the same user tries to register twice
- **Public authentication key**: the user can later prove they know the secret
  `s_l` corresponding to `nul_l` (e.g., via a Schnorr signature)

### One-out-of-many proof — Bootle/Groth 2015

A zero-knowledge proof that says: "I know the secret values that open one of
the 1024 commitments in this set — but I won't tell you which one."

More precisely, the prover demonstrates knowledge of an index `j` and opening
values `(s_1..s_L, k)` such that `set[j] = k·g + s_1·h_1 + ... + s_L·h_L`,
without revealing `j` or any of the secret values. The verifier is convinced
the prover is a legitimate member of the set but learns nothing about which
member they are.

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
| **Anonymity set (Λ)** | Fixed-size batch of 1024 commitments; once full the set is sealed (frozen forever) and serves as the ring for ZK proofs |
| **vtxo-tree** | Binary tree of pre-signed Bitcoin transactions; root is broadcast on-chain, 1024 leaves hold one P2TR output per user's Φ |

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
│  Registry (veiled::registry)                                     │
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

### Anonymity set sealing and Bitcoin anchoring

When an anonymity set reaches its capacity (**1024** commitments), it is
**sealed** — frozen permanently. No commitments can be added or removed after
sealing. New registrations go into a fresh set. This immutability is critical:
the Bootle/Groth ZK proof is generated against a specific fixed list of 1024
commitments. If the list could change between proof generation and verification,
the proof would be invalid.

Sealed sets are anchored on Bitcoin via a **vtxo-tree**: a binary tree of
pre-signed transactions where only the root is broadcast on-chain. The root
UTXO cryptographically commits to all 1024 leaf outputs. Each leaf is a P2TR
output locked to one user's commitment Φ (which is already a valid secp256k1
public key). This replaces the Ethereum smart contract from the ASC paper —
instead of storing 1024 entries in EVM storage, a single Bitcoin UTXO anchors
the entire set.

---

## Project layout

```
veiled/
├── Cargo.toml                        # single package with lib + 2 binaries
├── src/
│   ├── lib.rs                        # crate root (pub mod core, registry, vtxo_tree)
│   ├── core/                         # cryptographic primitives & shared types
│   │   ├── mod.rs                    # public API re-exports
│   │   ├── crs.rs                    # CRS setup, multi-value Pedersen commitment, HashToCurve generators
│   │   ├── credential.rs             # MasterCredential (Phase 1) + RegisteredIdentity (Phase 2) + ServiceRegistration (Phase 3)
│   │   ├── child_credential.rs       # child secret key + pseudonym derivation (Phase 3)
│   │   ├── service_proof.rs          # adapted Bootle/Groth proof for multi-value commitments (Phase 3)
│   │   ├── nullifier_v2.rs           # HKDF per-verifier nullifier derivation + public nullifiers
│   │   ├── nullifier.rs              # legacy SHA256(pub_key || name) nullifier (backward compat)
│   │   ├── commitment.rs             # single-value Pedersen commit (used by legacy proof)
│   │   ├── proof.rs                  # prove_membership / verify_membership (legacy Bootle/Groth)
│   │   └── types.rs                  # MasterSecret, ChildRandomness, BlindingKey, Nullifier, Commitment, Name, FriendlyName, ...
│   ├── registry/                     # HTTP registry server with SQLite persistence
│   │   ├── mod.rs
│   │   ├── server.rs                 # axum router + AppState
│   │   ├── db.rs                     # SQLite read/write (write-through)
│   │   ├── store.rs                  # in-memory state (anonymity sets + nullifier index)
│   │   ├── error.rs                  # AppError → HTTP responses
│   │   ├── bitcoin_anchor.rs         # vtxo-tree anchoring for sealed anonymity sets
│   │   └── routes/
│   │       ├── register.rs           # POST /api/v1/register + POST /api/v1/register-identity
│   │       ├── has.rs                # POST /api/v1/has
│   │       ├── sets.rs               # GET  /api/v1/sets[/:id]
│   │       └── verify.rs             # POST /api/v1/verify
│   ├── vtxo_tree/                    # Bitcoin vtxo-tree (pre-signed tx tree for 1024 users)
│   │   ├── mod.rs
│   │   ├── tree.rs
│   │   ├── types.rs
│   │   └── tx.rs
│   └── bin/
│       ├── registry.rs               # registry server entry point
│       └── cli.rs                    # command-line client
├── examples/
│   ├── credentials.rs                # key generation + credential derivation
│   ├── pedersen.rs                   # Pedersen commitment properties
│   └── membership_proof.rs           # full prove/verify over a 1024-element set
└── tests/
    ├── api.rs                        # HTTP integration tests
    ├── integration.rs                # vtxo-tree construction tests
    └── e2e.rs                        # end-to-end Bitcoin regtest tests
```

---

## Running the registry server

```bash
cargo run --bin veiled-registry
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
cargo run --example credentials

# Pedersen commitment properties including homomorphic addition (instant)
cargo run --example pedersen

# Full 1024-element anonymity set: prove + verify (~2–5 s release, ~90 s debug)
cargo run --example membership_proof --release
```

---

## Testing

```bash
cargo test                      # all 100 tests
cargo test -- --skip proof      # fast: skip slow proof tests (~40s)
```

Test coverage:
- **core** (76 unit tests): CRS generator independence and determinism; multi-value Pedersen commitment properties; FriendlyName commitment; HKDF nullifier derivation (determinism, uniqueness, cross-service independence); public nullifier validity; MasterCredential creation and recomputation; RegisteredIdentity index determination; child credential derivation; service registration proof (multi-generator Bootle/Groth); full Phase 1+2+3 flow
- **registry** (12 unit + 12 integration tests): store registration (single + multi-nullifier), duplicate rejection (atomic), set rollover, DB persistence round-trips, Bitcoin anchor (commitment-to-user, vtxo-tree construction, CRS-to-anchor flow); all HTTP endpoints exercised via `axum::Router::oneshot`
- **vtxo-tree** (12 tests): tree construction, value conservation, branch integrity, P2TR leaf outputs, determinism, full 1024-user scale

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
- [x] Phase 3: Service registration with adapted multi-generator Bootle/Groth proof, child credentials, pseudonyms, Schnorr π_value
- [ ] Phase 4: Anonymous authentication protocol
