# Scenario: Anonymous Authentication with CRS-ASC on Bitcoin

This document walks through the CRS-ASC protocol end-to-end, showing how a
user creates a master credential, registers on Bitcoin, and authenticates
with service providers — all while maintaining automatic cross-service
unlinkability.

---

## Setup: The CRS exists

A trusted setup has already produced the Common Reference String:

```
crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)
```

Where:
- `g` = HashToCurve("CRS-ASC-generator-0") — base generator
- `h_l` = HashToCurve("CRS-ASC-generator-{l}") — per-provider generators
- `v_l` = service provider names (e.g., "twitter.com", "github.com")
- L service providers are registered
- The Identity Registry (IdR) is initialized with set size N = 1024

Every user and service provider has access to the CRS.

---

## Phase 1 — Alice creates her master credential (local, offline)

Alice generates three secrets locally. Nothing touches the network.

```
sk ←$ {0,1}^256     # root secret for nullifier derivation
r  ←$ {0,1}^256     # child credential randomness
k  ←$ Z_q           # Pedersen blinding key
```

She derives L nullifier scalars and her master identity:

```
for l = 1..L:
  s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")   # per-service scalar
  nul_l = s_l · g                                        # public nullifier (group element)

Φ = k·g + s_1·h_1 + ... + s_L·h_L                       # multi-value Pedersen commitment
```

Alice's master credential: `(Φ, sk, r, k)`

She stores only `(sk, r, k)` — about 96 bytes. `Φ` can be recomputed anytime
from these secrets and the CRS.

**Key property**: Each `s_l` is derived from the same `sk` but with a different
service name as HKDF salt. HKDF's unlinkability guarantee means `s_1` and `s_2`
are computationally indistinguishable from independent random values — even
knowing both service names.

---

## Phase 2 — Alice registers on Bitcoin

Alice posts her master identity `Φ` to the Identity Registry:

```
POST /api/v1/register-identity
{
  "commitment": "<Φ as 66 hex chars>",
  "nullifiers": ["<s_1 as 64 hex>", "<s_2 as 64 hex>", ..., "<s_L as 64 hex>"]
}
```

The registry:
1. Checks ALL L nullifiers atomically — if any has been seen, rejects with **409 Conflict**
2. Appends Φ to the current anonymity set `Λ_d`
3. Inserts all nullifiers into the global index
4. Persists to SQLite

Alice waits for her anonymity set to fill to N = 1024 users. Once sealed:
- The set is anchored on Bitcoin via a vtxo-tree
- Each Φ (33-byte compressed secp256k1 point) becomes a P2TR leaf key
- The set is frozen and will never change

Alice determines her index `j` by finding her `Φ` in the list, and stores:

```
(Φ_j, sk, r, k, d̂, Λ_{d̂})

Where:
  Φ_j    = her master identity
  j      = her index within Λ_{d̂}
  d̂      = which anonymity set she's in
  Λ_{d̂} = the frozen set [Φ_1, ..., Φ_1024]
```

---

## What the anonymity set looks like

```
Λ = [Φ_1,         Φ_2,         ..., Φ_j,         ..., Φ_1024       ]
   = [k_1·g + Σ s_{1,l}·h_l,  k_2·g + Σ s_{2,l}·h_l,  ...,  k_N·g + Σ s_{N,l}·h_l]
```

Each Φ_i is an independent multi-value Pedersen commitment from a different user.
No two users share any secret values. The list is frozen on Bitcoin.

---

## What each party learns

| Party | Learns | Does NOT learn |
|---|---|---|
| CRS (public) | Group generators, service names | Nothing secret |
| Registry (at registration) | Φ (commitment), all L nullifier scalars | sk, r, k, or which service Alice will use |
| Bitcoin (on-chain) | Φ as a P2TR leaf key in the vtxo-tree | Nothing about the commitment's contents |
| Service provider l (at auth) | `nul_l = s_l · g` (public nullifier for their service) | Nullifiers for other services, sk, k |
| Two colluding services l, m | Their own `nul_l`, `nul_m` | Cannot link them — HKDF unlinkability is a protocol property |

---

## Cross-service unlinkability — automatic, not manual

In the CRS-ASC protocol, cross-service unlinkability is a **cryptographic guarantee**,
not a manual workaround:

```
s_1 = HKDF(sk, "twitter.com")  →  nul_1 = s_1 · g
s_2 = HKDF(sk, "github.com")   →  nul_2 = s_2 · g
```

- Twitter sees `nul_1`, GitHub sees `nul_2`
- `nul_1` and `nul_2` are computationally unlinkable
- Even if Twitter and GitHub collude and compare all their nullifiers,
  they cannot determine that `nul_1` and `nul_2` belong to the same user
- Alice does NOT need separate key pairs per service — one `sk` handles all services

This is a fundamental improvement over the old protocol where the user had to
manually generate separate key pairs per relationship.

---

## Sybil resistance

Sybil resistance operates at two levels:

### At registration (global)
All L nullifiers are checked atomically. The same master secret `sk` always
produces the same set of nullifiers, so a user cannot register twice.

### Per service (local)
Each service provider maintains a list of seen public nullifiers `nul_l = s_l · g`.
The same user always produces the same `nul_l` for service l, so a user cannot
create two accounts on the same service.

The nullifier scalar `s_l` serves double duty:
- **Sybil-resistance token**: unique per (master identity, service)
- **Public authentication key**: the user can prove knowledge of `s_l`

---

## Security properties

### Why Eve cannot impersonate Alice

| Eve's attempt | Result |
|---|---|
| Register with Alice's sk | All L nullifiers would match → **409 Conflict** |
| Register with a different sk | Different nullifiers, different Φ — not Alice's identity |
| Forge a proof for Alice's Φ | Requires knowing k and all s_l — discrete log is infeasible |
| Replay Alice's proof to a different service | Different service sees different `nul_l` — proof doesn't transfer |

### Binding property of the commitment

Given `Φ = k·g + s_1·h_1 + ... + s_L·h_L`, the only way to produce a valid
opening `(s_1', ..., s_L', k')` is if `(s_1', ..., s_L', k') = (s_1, ..., s_L, k)`
— up to negligible probability under the discrete log assumption.

This means: you cannot "open" position l to two different values. One master
secret, one set of nullifiers, one identity per service — cryptographically enforced.

### Blinding key security

If Alice's blinding key `k` is ever exposed, her commitment Φ can be "opened" —
an attacker who knows k and observes Φ could potentially recover the nullifier
scalars. The blinding key must be kept secret and stored securely.

### Bitcoin anchoring guarantees

Once a sealed set is anchored on Bitcoin via a vtxo-tree:
- The set is immutable — backed by Bitcoin's proof of work
- Each commitment is locked to a P2TR output — spending requires proving
  knowledge of the commitment opening via the ASC proof protocol
- The on-chain anchor replaces the Ethereum smart contract from the paper

---

## What veiled does NOT yet provide

| Property | Status |
|---|---|
| Phase 3: Service-specific credential derivation | Planned |
| Phase 3: Bootle/Groth proof adapted for multi-value commitments | Planned |
| Phase 3: Single composite proof (membership + nullifier authenticity) | Planned |
| Phase 4: Full anonymous authentication protocol | Planned |
| Proof expiry / revocation | Not in scope |
