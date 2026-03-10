# veiled vs. ASC — Design Comparison

This document compares veiled's CRS-ASC implementation against the protocol
described in the Anonymous Self-Credentials (ASC) paper. It explains where the
two designs converge, where they diverge, and why.

---

## The ASC model (brief summary)

The ASC paper describes a protocol with the following roles and properties:

- **Prover** — holds a *master credential*: `(Φ, sk, r, k)`.
- **Verifier** — identified by a unique *service name* `v_l` registered in the CRS.
- **Anonymity set** — the publicly available set of all master identities `Λ = [Φ_1, ..., Φ_N]`.
- **CRS** — Common Reference String: `(g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)`.

When a prover wants to authenticate with a verifier:

1. The CRS defines L+1 independent generators and L service providers.
2. The prover derives L per-service nullifiers via `s_l = HKDF(sk, v_l)`.
3. The master identity `Φ = k·g + s_1·h_1 + ... + s_L·h_L` is a multi-value Pedersen commitment.
4. The prover proves membership in the anonymity set via a zero-knowledge proof.
5. Each verifier sees a different nullifier `nul_l = s_l · g` — unlinkability is automatic.

Key properties the paper achieves:
- **Sybil resistance** — one nullifier per service per master identity.
- **Automatic cross-service unlinkability** — different services derive different nullifiers from the same master identity; they cannot collude to link a prover.
- **Multi-value commitment** — L nullifier values committed under a single blinding key.
- **Bitcoin-native** — veiled implements this on Bitcoin via vtxo-trees (pre-signed
  transaction trees where each commitment becomes a P2TR leaf key), replacing the
  Ethereum smart contracts described in the original paper.

---

## What veiled shares with ASC

| Concept | Implementation |
|---|---|
| CRS with independent generators | `g, h_1..h_L` via HashToCurve with DST `"CRS-ASC-v1"` |
| Multi-value Pedersen commitment | `Φ = k·g + s_1·h_1 + ... + s_L·h_L` |
| HKDF per-verifier nullifier derivation | `s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")` |
| Public nullifiers as group elements | `nul_l = s_l · g` (33-byte compressed secp256k1 point) |
| MasterCredential tuple | `(Φ, sk, r, k)` — user stores ~96 bytes |
| RegisteredIdentity with frozen set | `(Φ_j, sk, r, k, d̂, Λ_{d̂})` — user keeps the full set locally for ZK proof generation |
| Anonymity set of size 1024 | Same ring size (power of 2 for efficient vtxo-tree binary structure) |
| Automatic cross-service unlinkability | Same (sk, different v_l) → different nullifiers |
| Bootle/Groth one-out-of-many proof | Same 2015 paper; M=10, N=1024, 878-byte proof |
| Blinding key stays client-side | Neither the registry nor any verifier sees k |

---

## Differences — quick reference

| Property | ASC (paper) | veiled (implementation) |
|---|---|---|
| Identity registry | Ethereum smart contract `IdR` | Bitcoin vtxo-tree + HTTP registry |
| On-chain anchor | EVM transaction | P2TR leaf keys in pre-signed tx tree |
| Registration fee | Gas fee | Bitcoin tx fee |
| Set filling trigger | Smart contract event | Registry monitors set capacity |
| Proof composition | Single composite proof | Two-step: `/has` then `/verify` (to be unified in Phase 3) |
| Legacy single-nullifier mode | Not in paper | Kept for backward compatibility |
| Registry dependency | Optional at verify time | Required (stores sets, runs verifier) |

---

## Detailed comparison

### 1. Bitcoin vs. Ethereum identity registry

**ASC (paper):**
The identity registry `IdR` is an Ethereum smart contract. Users call
`addID(Φ)` to register (costs gas), and the contract emits events when sets
fill. Each commitment is stored in EVM contract storage.

**veiled:**
The identity registry is a combination of:
- An HTTP API (`POST /api/v1/register-identity`) for off-chain registration
- SQLite for persistence between server restarts
- Bitcoin vtxo-trees for on-chain anchoring of sealed sets

When an anonymity set reaches 1024 commitments and is sealed, it is anchored
on Bitcoin via a **vtxo-tree** — a binary tree of pre-signed transactions
where only the root is broadcast on-chain. Each of the 1024 leaves is a P2TR
output whose internal key is a user's commitment Φ. This works natively
because Φ is already a valid 33-byte compressed secp256k1 public key — no
encoding or wrapping needed.

**Consequence:** veiled achieves the same on-chain anchoring guarantees as
the ASC paper but on Bitcoin's UTXO model. A single on-chain UTXO anchors
1024 identities, which is significantly more efficient than storing 1024
entries individually in an Ethereum smart contract.

---

### 2. Nullifier derivation — now matching the paper

**ASC (paper):**
```
s_l = HKDF(sk, salt=v_l)
nul_l = s_l · g
```

**veiled (current):**
```
s_l = HKDF-SHA256(IKM=sk, salt=v_l, info="CRS-ASC-nullifier")
nul_l = s_l · g
```

This now matches the paper. The nullifier scalar `s_l` is derived via HKDF
with the service name as salt, producing automatic cross-service unlinkability.
The public nullifier `nul_l = s_l · g` is a group element serving as both
a Sybil-resistance token and a public authentication key.

**Note:** veiled also retains the legacy `SHA256(pub_key || name)` nullifier
in `nullifier.rs` for backward compatibility with the original single-value
commitment scheme.

---

### 3. Multi-value Pedersen commitment — matching the paper

**ASC (paper):**
```
Φ = k·g + s_1·h_1 + ... + s_L·h_L
```

**veiled (current):**
```
Φ = k·g + s_1·h_1 + ... + s_L·h_L
```

Exact match. The CRS provides L+1 independent generators derived via
HashToCurve. The commitment binds all L nullifier values under a single
blinding key k. The user only needs to store (sk, r, k) — 96 bytes — since
Φ can be recomputed from these values and the CRS.

---

### 4. Master credential and registered identity — matching the paper

**ASC (paper):**
```
Phase 1: credential = (Φ, sk, r, k)
Phase 2: registered = (Φ_j, sk, r, k, d̂, Λ_{d̂})
```

**veiled (current):**
```
MasterCredential { phi, sk, r, k }
RegisteredIdentity { credential, set_id, index, anonymity_set }
```

Exact match. `MasterCredential::create()` performs Phase 1 locally.
`RegisteredIdentity::new()` performs Phase 2 by finding the user's index j
in the frozen anonymity set.

---

### 5. Atomic multi-nullifier registration

**ASC (paper):**
Registration submits Φ and all L nullifiers are checked.

**veiled:**
`POST /api/v1/register-identity` accepts the commitment and all L nullifiers.
The check is atomic: if ANY nullifier has been seen before, the entire
registration is rejected with 409 Conflict. No partial registrations.

---

### 6. Proof composition (current limitation)

**ASC (paper):**
The membership proof and nullifier-authenticity proof are combined into a
single composite proof. The prover sends one message that simultaneously proves:
(a) "I am a member of this anonymity set" and (b) "this nullifier authentically
belongs to my committed identity."

**veiled (current):**
The Bootle/Groth proof currently operates on the old single-value commitment
scheme (`C = r·G + v·H`). It has not yet been adapted to work with the new
multi-value commitments (`Φ = k·g + s_1·h_1 + ... + s_L·h_L`). This
adaptation — making the ZK proof open a specific position `l` within the
multi-value commitment to reveal `nul_l = s_l · g` without revealing the
other positions — is the main task for Phase 3.

---

### 7. Where Sybil resistance lives

**ASC (paper):**
Each verifier maintains its own local nullifier list. When a user presents
`nul_l` to service l, the service checks its list — if `nul_l` is already
there, the user has already registered with that service. The verifier is
self-sovereign and needs no central authority.

**veiled:**
Sybil resistance operates at two levels:
1. **Global (at registration)**: the central registry maintains a global index
   of all L×N nullifier scalars. When a user registers, all L nullifiers are
   checked atomically. If any one has been seen before, the entire registration
   is rejected. This prevents the same master secret from being used twice.
2. **Per-service (at authentication)**: each service provider can additionally
   maintain its own local list of seen public nullifiers `nul_l = s_l · g`.
   Since the same `(sk, v_l)` always produces the same `nul_l`, a service can
   detect duplicate accounts independently.

---

## What veiled achieves from the ASC paper (Phases 0–2)

| Property | Status |
|---|---|
| CRS with independent generators via HashToCurve | Implemented |
| Multi-value Pedersen commitment `Φ = k·g + Σ s_l·h_l` | Implemented |
| HKDF per-verifier nullifier derivation | Implemented |
| Public nullifiers `nul_l = s_l · g` | Implemented |
| Automatic cross-service unlinkability | Implemented |
| MasterCredential `(Φ, sk, r, k)` | Implemented |
| RegisteredIdentity with frozen anonymity set | Implemented |
| Multi-nullifier atomic registration | Implemented |
| Bitcoin on-chain anchoring via vtxo-tree | Implemented |
| Anonymity set capacity N=1024 | Implemented |

## What remains (Phases 3+)

| Property | Status |
|---|---|
| Bootle/Groth proof adapted for multi-value commitments | Planned (Phase 3) |
| Service-specific credential derivation | Planned (Phase 3) |
| Single composite proof (membership + nullifier authenticity) | Planned (Phase 3) |
| Anonymous authentication protocol | Planned (Phase 4) |
