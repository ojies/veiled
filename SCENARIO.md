# Scenario: Anonymous Authentication with CRS-ASC on Bitcoin

This document walks through the CRS-ASC protocol end-to-end, showing how a
user creates a master credential, registers on Bitcoin, and authenticates
with service providers — all while maintaining automatic cross-service
unlinkability.

---

## Setup: The CRS exists (Phase 0)

Before anything else happens, a trusted setup produces the **Common Reference
String (CRS)** — a set of public parameters that everyone in the system shares:

```
crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)
```

What's in it:
- **g** = a base generator point on secp256k1, derived by hashing the string
  `"CRS-ASC-generator-0"` to a curve point. Nobody knows its discrete log.
- **h_1..h_L** = L additional generator points, one per service provider, each
  derived from `"CRS-ASC-generator-{l}"`. These are provably independent from
  g and from each other (NUMS — Nothing Up My Sleeve).
- **v_1..v_L** = the names of L registered service providers (e.g., "twitter.com",
  "github.com"). These strings double as HKDF salts for nullifier derivation.
- **G_auth_1..G_auth_L** = credential generators for each service provider.
- The Identity Registry is initialized with set size N = 1024.

The CRS is public — every user and service provider has access to it.
The key property of the generators is that nobody knows the ratio between any
two of them. If someone could compute `γ` such that `h_1 = γ·g`, they could
forge commitments. HashToCurve prevents this.

---

## Phase 1 — Alice creates her master credential (local, offline)

Everything in this phase happens on Alice's device. Nothing touches the network.

### Step 1: Generate three secrets

```
sk ←$ {0,1}^256     # root secret — derives all nullifiers (the "master key")
r  ←$ {0,1}^256     # child credential randomness — for deriving service-specific auth keys later
k  ←$ Z_q           # Pedersen blinding key — hides everything inside the commitment
```

Why three separate secrets? Each serves a different purpose and keeping them
independent prevents information leakage between protocol layers. If `sk` were
reused for blinding, learning the blinding factor would reveal nullifier
information.

### Step 2: Derive L nullifier scalars (one per service)

```
for l = 1..L:
  s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")
```

HKDF (HMAC-based Key Derivation Function) takes Alice's master secret `sk` and
combines it with each service provider's name `v_l` to produce a different
32-byte scalar. For example:

- `s_1 = HKDF(sk, "twitter.com")` → a 32-byte number for Twitter
- `s_2 = HKDF(sk, "github.com")` → a completely different 32-byte number for GitHub

Even though both come from the same `sk`, HKDF guarantees the outputs are
computationally indistinguishable from independent random values. This is what
makes cross-service unlinkability automatic — nobody can tell that `s_1` and
`s_2` came from the same secret.

Each scalar also has a public form — the **public nullifier**:

```
nul_l = s_l · g    (scalar × generator point = a new curve point)
```

This is a one-way operation. Service provider l will see `nul_l` but cannot
recover `s_l` from it (that would require solving the discrete log problem on
secp256k1).

### Step 3: Compute the master identity commitment

```
Φ = k·g + s_1·h_1 + s_2·h_2 + ... + s_L·h_L
```

This packs all L nullifier scalars into a single 33-byte elliptic curve point.
The blinding key `k` hides everything — without knowing `k`, an observer sees
only a random-looking point on secp256k1.

**The result**: Alice's master credential is `(Φ, sk, r, k)`.

She only needs to store `(sk, r, k)` — about **96 bytes**. The commitment `Φ`
can always be recomputed from these three secrets plus the public CRS, since
all L nullifier scalars are deterministically derived from `sk` and the service
names in the CRS.

---

## Phase 2 — Alice registers on Bitcoin

### Step 1: Post Φ to the Identity Registry

Alice sends her master identity commitment and all L nullifier scalars:

```
POST /api/v1/register-identity
{
  "commitment": "<Φ as 66 hex chars>",
  "nullifiers": ["<s_1 as 64 hex>", "<s_2 as 64 hex>", ..., "<s_L as 64 hex>"]
}
```

The registry checks ALL L nullifiers atomically — if any single one has been
seen before (meaning someone with the same master secret already registered),
the entire registration is rejected with **409 Conflict**. This is Sybil
resistance: one master secret → one identity, enforced at registration time.

If accepted, the registry appends Φ to the current anonymity set and persists
everything to SQLite.

### Step 2: Wait for the anonymity set to fill

Alice's Φ is now in an anonymity set `Λ_d` along with other users'
commitments. The set needs exactly 1024 members before it can be used.

Why wait? Two reasons:
1. **Privacy**: an anonymity set with 5 members offers almost no privacy.
   With 1024, an observer cannot tell which of the 1024 users is proving
   membership — that's the "anonymity" in "anonymity set."
2. **Proof correctness**: the Bootle/Groth ZK proof is generated against a
   fixed, complete list. A partially filled set could change between proof
   generation and verification, invalidating the proof.

### Step 3: Set seals and is anchored on Bitcoin

Once 1024 users have registered, the set is **sealed** — frozen permanently.
It is then anchored on Bitcoin using a **vtxo-tree** (virtual transaction
output tree):

```
                    [Root TX]  ← broadcast on-chain (single UTXO)
                   /          \
             [Branch TX]    [Branch TX]
             /        \      /        \
           ...       ...   ...       ...
          /   \
      [Leaf]  [Leaf]  ...  [Leaf]   ← 1024 P2TR outputs, one per user
       Φ_1     Φ_2          Φ_1024
```

- Only the **root transaction** is broadcast on Bitcoin. This is one UTXO
  that cryptographically commits to the entire tree of 1024 users.
- Each **leaf** is a P2TR (Pay-to-Taproot) output. The internal key of each
  leaf is the user's commitment Φ. This works because Φ is a 33-byte
  compressed secp256k1 point — which is already a valid Bitcoin public key.
- The interior nodes are **pre-signed connector transactions** that link the
  root to the leaves. They are not broadcast unless a user needs to exit
  unilaterally (claim their leaf output on-chain independently).
- This replaces the Ethereum smart contract `IdR` from the ASC paper. Instead
  of paying gas to store 1024 entries in EVM storage, a single Bitcoin UTXO
  anchors the entire identity set.

### Step 4: Alice stores her registered identity

Alice downloads the complete frozen set and finds her position:

```
(Φ_j, sk, r, k, d̂, Λ_{d̂})

Where:
  Φ_j    = her specific master identity (one of the 1024)
  j      = her index within the set (e.g., position 417 out of 1024)
  d̂      = which anonymity set she's in (sets are numbered sequentially)
  Λ_{d̂} = the complete frozen list [Φ_1, ..., Φ_1024]
```

Alice needs the full set stored locally because the Bootle/Groth ZK proof
requires the prover to have the entire ring of 1024 commitments. To prove
"I know the opening to one of these 1024 commitments without revealing which
one," you need to know all 1024 to construct the proof.

---

## What the anonymity set looks like

```
Λ = [Φ_1,         Φ_2,         ..., Φ_j (Alice),   ..., Φ_1024       ]
   = [k_1·g + Σ s_{1,l}·h_l,  k_2·g + Σ s_{2,l}·h_l,  ...,  k_N·g + Σ s_{N,l}·h_l]
```

Each `Φ_i` is an independent multi-value Pedersen commitment from a different
user, each with their own secrets `(sk_i, k_i)`. No two users share any secret
values. To an outside observer, the set is just 1024 random-looking 33-byte
curve points — there is no way to tell which one belongs to Alice without
knowing her secrets.

The list is frozen on Bitcoin (anchored via the vtxo-tree root transaction)
and will never change.

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

In the CRS-ASC protocol, cross-service unlinkability is a **cryptographic
guarantee**, not something the user has to manage manually:

```
s_1 = HKDF(sk, "twitter.com")  →  nul_1 = s_1 · g   (what Twitter sees)
s_2 = HKDF(sk, "github.com")   →  nul_2 = s_2 · g   (what GitHub sees)
```

- Twitter sees `nul_1`, GitHub sees `nul_2` — two completely different points
- Even if Twitter and GitHub collude and compare all their nullifiers across
  all their users, they cannot determine that `nul_1` and `nul_2` came from
  the same person. HKDF's security proof guarantees this.
- Alice does NOT need separate key pairs per service — one master secret `sk`
  handles all L services, yet produces L unlinkable identities

Compare with the old protocol: there, Alice would need to manually generate a
separate `(pub_key, blinding_key)` pair for every service relationship and
register each one separately. With CRS-ASC, she registers once and gets
automatic unlinkability for all L services built into the protocol.

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
- **Immutability**: the set cannot be altered — the root transaction is
  confirmed on Bitcoin's blockchain, backed by proof of work. Changing any
  commitment would require re-mining blocks.
- **Commitment locking**: each commitment Φ is the internal key of a P2TR
  leaf output. To spend that output (claim the coins), a user must prove
  they know the opening values `(k, s_1..s_L)` — this is where the ASC
  zero-knowledge proof comes in.
- **Efficiency**: one on-chain UTXO anchors 1024 identities, compared to
  the ASC paper's approach of storing each identity in an Ethereum smart
  contract (which costs gas per entry).

---

## Phase 3 — Alice authenticates with a service (service registration proof)

Phase 3 builds on the master credential and registered identity to enable
Alice to prove membership in her anonymity set and authenticate with a
specific service — all without revealing which commitment is hers.

### FriendlyName — human-readable identity bound to Φ

Before or during credential creation, Alice can choose a **FriendlyName** —
a human-readable identifier (max 255 bytes) that is cryptographically bound
inside her commitment via a dedicated generator `h_name`:

```
name_scalar = SHA256(friendly_name)
Φ = k·g + s_1·h_1 + ... + s_L·h_L + name_scalar·h_name
```

The CRS provides `h_name = HashToCurve("CRS-ASC-generator-name")`, an
independent NUMS generator alongside `g` and `h_1..h_L`. Once committed,
the friendly name cannot be changed without breaking the cryptographic
binding in Φ.

### Child credentials — per-service authentication keys

Alice derives a **child secret key** and **pseudonym** for each service `l`:

```
csk_l = HKDF(r, salt=v_l, info="CRS-ASC-child-secret-key")
ϕ_l   = csk_l · g    (pseudonym — Alice's public identity at service l)
```

Key properties:
- `r` (child randomness) is independent from `sk` (master secret) — auth
  keys don't leak nullifier information
- Each service gets a different `csk_l` because the salt changes
- The pseudonym `ϕ_l` is unlinkable across services (same HKDF guarantee
  as nullifiers)
- Alice stores only `(sk, r, k)` (~96 bytes); pseudonyms are recomputed
  on demand

### Service registration proof — composite membership + nullifier authenticity

To authenticate with service `l`, Alice generates a **ServiceRegistrationProof**
that simultaneously proves two things:

1. **Membership**: "I know the opening of one of the 1024 commitments in
   anonymity set `Λ_{d̂}`, without revealing which one."
2. **Nullifier authenticity**: "The public nullifier `nul_l = s_l · g` I'm
   presenting is correctly derived from my committed identity."

The proof works by **shifting** each commitment in the anonymity set:

```
D[i] = Φ_i - s_l·h_l    for all i = 1..1024
```

At Alice's index `j`, this cancels the `l`-th term:

```
D[j] = k·g + Σ_{m≠l} s_m·h_m + name_scalar·h_name
```

The adapted Bootle/Groth proof then proves knowledge of the opening
`(k, s_1, ..., s_{l-1}, s_{l+1}, ..., s_L, name_scalar)` to one of the
1024 shifted commitments `D[i]`, using **L+1 active generators**:
`g`, `h_m` for `m ≠ l`, and `h_name`.

A **Schnorr proof `π_value`** additionally proves that `nul_l = s_l · g`
is correctly formed: the prover shows knowledge of `s_l` such that the
publicly revealed `nul_l` equals `s_l` times the base generator.

### Proof structure and size

```
ServiceRegistrationProof {
  π_membership:
    A, B, C, D     — 4 aggregate bit commitments (132 bytes)
    E_poly         — M=10 polynomial decomposition points (330 bytes)
    f              — M=10 scalar evaluations (320 bytes)
    z_a, z_c       — 2 bit blinding responses (64 bytes)
    z_responses    — (L+1) multi-generator polynomial responses

  π_value (Schnorr):
    schnorr_r      — nonce commitment R = t·g (33 bytes)
    schnorr_s      — response s = t + e·s_l (32 bytes)
}

Total: 911 + (L+1)×32 bytes
Example: L=4 providers → 911 + 160 = 1,071 bytes
```

The Fiat-Shamir challenge binds the proof to the CRS, pseudonym, public
nullifier, service index, set ID, shifted anonymity set, and all
commitments — preventing replay across services or sets.

### What the service provider sees

After verification, service `l` learns:
- `nul_l = s_l · g` — Alice's unique nullifier for this service (Sybil resistance)
- `ϕ_l = csk_l · g` — Alice's pseudonym (her public identity at service `l`)
- That Alice is a valid member of anonymity set `d̂`

Service `l` does **not** learn:
- Which of the 1024 commitments is Alice's
- Alice's nullifiers for any other service
- Alice's blinding key `k`, master secret `sk`, or friendly name

---

## What veiled provides

| Property | Status |
|---|---|
| CRS with independent generators via HashToCurve | Implemented (Phase 0) |
| Multi-value Pedersen commitment `Φ = k·g + Σ s_l·h_l + name_scalar·h_name` | Implemented (Phase 0) |
| HKDF per-verifier nullifier derivation | Implemented (Phase 1) |
| Public nullifiers `nul_l = s_l · g` | Implemented (Phase 1) |
| MasterCredential `(Φ, sk, r, k)` | Implemented (Phase 1) |
| FriendlyName bound in commitment via `h_name` | Implemented (Phase 1) |
| RegisteredIdentity with frozen anonymity set | Implemented (Phase 2) |
| Multi-nullifier atomic registration | Implemented (Phase 2) |
| Bitcoin on-chain anchoring via vtxo-tree | Implemented (Phase 2) |
| Adapted Bootle/Groth proof for multi-value commitments | Implemented (Phase 3) |
| Service-specific child credentials and pseudonyms | Implemented (Phase 3) |
| Composite proof (membership + nullifier authenticity via Schnorr `π_value`) | Implemented (Phase 3) |
| Automatic cross-service unlinkability | Implemented (Phases 1–3) |

## What veiled does NOT yet provide

| Property | Status |
|---|---|
| Phase 4: Full anonymous authentication protocol | Planned |
| Proof expiry / revocation | Not in scope |
