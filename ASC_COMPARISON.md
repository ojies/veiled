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
- **Bitcoin-native** — anchored on Bitcoin via vtxo-trees, not Ethereum smart contracts.

---

## What veiled shares with ASC

| Concept | Implementation |
|---|---|
| CRS with independent generators | `g, h_1..h_L` via HashToCurve with DST `"CRS-ASC-v1"` |
| Multi-value Pedersen commitment | `Φ = k·g + s_1·h_1 + ... + s_L·h_L` |
| HKDF per-verifier nullifier derivation | `s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")` |
| Public nullifiers as group elements | `nul_l = s_l · g` (33-byte compressed secp256k1 point) |
| MasterCredential tuple | `(Φ, sk, r, k)` — user stores ~96 bytes |
| RegisteredIdentity with frozen set | `(Φ_j, sk, r, k, d̂, Λ_{d̂})` |
| Anonymity set of size 1024 | Same ring size (power of 2 for vtxo-tree) |
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
`addID(Φ)` to register, and the contract emits events when sets fill.

**veiled:**
The identity registry is a combination of:
- An HTTP API (`POST /api/v1/register-identity`) for registration
- SQLite for persistence
- Bitcoin vtxo-trees for on-chain anchoring of sealed sets

Each sealed anonymity set of 1024 commitments is anchored on Bitcoin via a
vtxo-tree. Since commitments are valid 33-byte compressed secp256k1 points,
they map directly to P2TR leaf keys — no additional encoding needed.

**Consequence:** veiled achieves the same on-chain anchoring guarantees as
the ASC paper but on Bitcoin's UTXO model rather than Ethereum's account model.

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
single composite proof.

**veiled (current):**
The Bootle/Groth proof currently operates on the old single-value commitment
scheme. Adapting it to prove membership for multi-value commitments is planned
for Phase 3.

---

### 7. Where Sybil resistance lives

**ASC (paper):**
Each verifier maintains its own local nullifier list. The verifier is
self-sovereign.

**veiled:**
The central registry maintains a global nullifier index. At registration time,
all L nullifiers are checked. Additionally, each service provider can maintain
its own local `nul_l = s_l · g` list for per-service Sybil resistance.

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
