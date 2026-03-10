# veiled vs. ASC — Design Comparison

This document compares veiled's implementation against the protocol described
in the Anonymous Self-Credentials (ASC) paper.  It explains where the two
designs converge, where they diverge, and why some of those divergences are
intentional.

---

## The ASC model (brief summary)

The ASC paper describes a protocol with the following roles and properties:

- **Prover** — holds a *master credential*: a pair `(master_identity, master_secret_key)`.
- **Verifier** — identified by a unique *verifier identifier*.
- **Anonymity set** — the publicly available set of all master identities.

When a prover wants to register with a verifier she:

1. Generates a **pseudonym** (distinct from her master identity).
2. Sends the pseudonym and a **zero-knowledge proof** asserting she owns one
   of the master identities in the anonymity set — without revealing which one.
3. Derives a **nullifier** = `f(master_identity, verifier_identifier)`.
   The nullifier is unique per `(prover, verifier)` pair.
4. The verifier checks its own local nullifier list and accepts the pseudonym
   only if the nullifier has not been seen before, then adds it to the list.

Key properties the paper achieves:
- **Sybil resistance** — one pseudonym per verifier per master identity.
- **Automatic cross-verifier unlinkability** — different verifiers derive
  different nullifiers from the same master identity; they cannot collude to
  link a prover across services.
- **Pseudonym privacy** — verifiers learn pseudonyms, not master identities.
- **Single composite proof** — membership + nullifier authenticity are bundled
  into one message.

---

## What veiled shares with ASC

| Concept | Shared |
|---|---|
| Nullifier-based Sybil resistance | Both use a one-way tag to detect duplicate registrations |
| Pedersen commitment on secp256k1 | Same `C = r·G + v·H` formula from the `crypto-dbpoe` reference implementation |
| Bootle/Groth one-out-of-many proof | Same 2015 paper; M=10, N=1024, 878-byte proof |
| Anonymity set of size 1024 | Same ring size |
| Blinding key stays client-side | Neither the registry nor the verifier ever sees `r` |

---

## Differences — quick reference

| Property | ASC (paper) | veiled (implementation) |
|---|---|---|
| Nullifier input | `f(master_identity, verifier_id)` | `SHA256(pub_key \|\| name)` |
| Cross-verifier unlinkability | Automatic (protocol property) | Manual (use a different key pair per verifier) |
| Sybil resistance enforcer | Each verifier (local list) | Central registry (global nullifier set) |
| Pseudonym abstraction | Yes — verifier learns pseudonym, not master identity | No — prover sends `pub_key` directly to verifier |
| Anonymity set contents | Public master identities | Pedersen commitments (identities are hidden) |
| Proof composition | Single composite proof | Two-step: `/has` then `/verify` |
| Proof binding | Bound to `(master_identity, verifier_id)` | Bound to `(nullifier, set_id)` only |
| Replay prevention | Cryptographic (verifier_id in proof) | Operational (verifier must track accepted nullifiers) |
| Registry dependency | Optional — set can be distributed statically | Required — provides set contents, nullifier checks |

---

## Detailed differences

### 1. Nullifier derivation

**ASC:**
```
nullifier = f(master_identity, verifier_identifier)
```
The verifier contributes their own identifier to the derivation.  The same
master identity produces a completely different nullifier for every verifier —
unlinkability is baked into the formula.

**veiled:**
```
nullifier = SHA256(pub_key || name)
```
The prover chooses the `name`.  The verifier contributes nothing.  The same
`(pub_key, name)` pair always yields the same nullifier regardless of who the
verifier is.

**Consequence:** In ASC, cross-verifier unlinkability is a guaranteed protocol
property — no action required from the prover.  In veiled it requires the prover
to manually use a separate `(pub_key, blinding)` key pair per verifier
relationship (see [SCENARIO.md — Multi-party unlinkability](SCENARIO.md)).

---

### 2. Where Sybil resistance lives

**ASC:**
Each verifier maintains its own local nullifier list.  The verifier accepts a
pseudonym only if the nullifier is not already in that list, then adds it.  No
central registry is needed for Sybil resistance — every verifier is
self-sovereign.

**veiled:**
The central registry maintains a global `nullifiers: HashSet<Nullifier>`.  At
`POST /api/v1/register` it rejects any duplicate with **409 Conflict**.  The
verifier (Bob) trusts the registry's `"present": true` response without doing
any local duplicate checking.

**Consequence:** veiled requires the verifier to trust the registry.  A
compromised or colluding registry could silently allow duplicate registrations
or deny legitimate ones.  In ASC the verifier's Sybil check cannot be
subverted by any third party.

---

### 3. Pseudonym abstraction

**ASC:**
The prover generates a **pseudonym** — a separate identity from her master
credential — and presents it to the verifier along with the ZK proof.  The
verifier learns only the pseudonym.

**veiled:**
There is no pseudonym layer.  The prover sends her actual `pub_key` (the master
identity) directly to the verifier.  Bob knows exactly which master identity
belongs to Alice.

**Consequence:** This difference is **intentional** in the payment context.
Bob needs to know who Alice is in order to send her money.  The stronger
privacy guarantee of ASC (verifier never learns master identity) is not needed
here — and adding a pseudonym layer would introduce complexity without benefit.
For service-access use cases where the verifier should not learn the user's
real identity, veiled would need to be extended with a pseudonym mechanism.

---

### 4. Anonymity set contents

**ASC:**
The anonymity set contains the **public master identities** themselves.  The
set is published openly; provers prove membership in this public set.

**veiled:**
The anonymity set contains **Pedersen commitments** (`r·G + v·H`), not the
master identities.  The registry stores commitments, which cryptographically
hide both the nullifier (the identity hash) and the blinding key.

**Consequence:** veiled is actually *stronger* than ASC at the registry level.
The registry never learns any master identity — only commitments that are
computationally opaque without the blinding key.  In ASC, the registry (or
anyone who sees the public set) knows every master identity.  This is a
deliberate improvement over the paper model.

---

### 5. Single composite proof vs. two-step protocol

**ASC:**
The membership proof and the nullifier-authenticity proof are combined into a
**single composite proof** sent to the verifier in one message.

**veiled:**
The protocol is split across two HTTP endpoints:

1. `POST /api/v1/has` — Bob checks that Alice is registered and retrieves
   her nullifier.
2. `POST /api/v1/verify` — Bob submits a ZK membership proof and Alice's
   nullifier; the registry verifies the proof against the anonymity set.

**Consequence:** An extra round-trip is required.  This is a pragmatic
simplification: the registry acts as both the commitment store and the proof
verifier, so the split naturally maps to "look up the nullifier, then verify
the proof".  Combining both into one message would be a possible future
optimisation but adds no security benefit in this deployment model.

---

### 6. Proof binding and replay prevention

**ASC:**
The proof is bound to `(master_identity, verifier_identifier)`.  A proof
generated for verifier Bob cannot be replayed to verifier John — the
verifier_id in the proof hash would not match.

**veiled:**
The proof is bound to `(nullifier, set_id)` only.  The nullifier is
`SHA256(pub_key || name)`, not tied to any verifier identifier.  The same
proof can be verified by any party who has access to set `set_id`.

**Consequence:** Replay prevention is an **operational responsibility** in
veiled.  Bob must track which nullifiers he has already accepted and refuse
to honour the same nullifier a second time.  See
[SCENARIO.md — Replay](SCENARIO.md) for guidance.

---

### 7. Registry dependency

**ASC:**
The registry's role is limited to the initial bootstrapping of the public
master-identity set.  At proof time the verifier works locally — it holds the
public set and its own nullifier list, and needs no live registry call.

**veiled:**
The registry is a required online service:
- It stores all commitments and serves them to provers (`GET /api/v1/sets/:id`).
- It provides the nullifier lookup (`POST /api/v1/has`).
- It performs proof verification against the stored set (`POST /api/v1/verify`).
- It enforces global Sybil resistance.

**Consequence:** veiled has a stronger operational dependency on the registry.
If the registry is unavailable, provers cannot register and verifiers cannot
verify.  This is an acceptable trade-off in a centralised payment context but
would need re-architecting for fully decentralised deployments.

---

## Intentional departures from ASC

Some differences are deliberate design choices, not omissions:

| Departure | Reason |
|---|---|
| No pseudonym layer | In a payment context Bob needs to know who Alice is — pseudonym privacy is not required |
| Central registry for Sybil resistance | Simpler to operate; acceptable when the registry operator is trusted (e.g. a financial institution or a smart contract) |
| Commitments in the anonymity set (not raw identities) | Strictly stronger than the paper — registry learns nothing about member identities |
| Two-step protocol | Natural fit for the deployment model; composite proof adds complexity without security gain here |
| `pub_key || name` nullifier | Allows a single master identity to have multiple independent relationships by changing the name; keeps the protocol simple without a separate verifier-identifier infrastructure |

---

## What veiled does NOT provide (vs. ASC)

| Property | ASC | veiled |
|---|---|---|
| Automatic cross-verifier unlinkability | Yes — protocol property | No — manual key-per-relationship workaround |
| Verifier-local Sybil resistance | Yes — verifier is self-sovereign | No — depends on central registry |
| Pseudonym abstraction | Yes — verifier never sees master identity | No — prover sends `pub_key` directly |
| Single composite proof | Yes | No — two separate endpoints |
| Cryptographic replay prevention | Yes — verifier_id bound in proof | No — operational convention only |
| Registry-free operation (at verify time) | Yes | No — registry must be online |
