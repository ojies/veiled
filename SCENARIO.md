# Scenario: Bob sends Alice money

This document walks through the practical use of veiled in a peer-to-peer
payment context.  All commands use the CLI; all HTTP calls can be made with any HTTP client.

---

## The problem

Bob wants to send Alice money.  Alice gives Bob her `pub_key` and name.
Before sending, Bob wants two guarantees:

1. Alice is a **registered** identity — not a throwaway address invented
   on the spot.
2. Alice **controls** that identity — she is the one who registered it,
   not someone who merely knows her public details.

Veiled solves both with two endpoints.

---

## Step 1 — Alice registers (one time)

Alice generates her keys locally:

```bash
veiled generate-key
# pub_key:  <64 hex chars>
# blinding: <64 hex chars>
```

She saves them to a keyfile for reuse:

```bash
veiled save-key \
  --pub-key  <pub_key> \
  --blinding <blinding> \
  --out      alice-keys.json
```

She registers with the registry under her chosen name:

```bash
veiled register \
  --pub-key  <pub_key> \
  --name     alice \
  --blinding <blinding>
# registered → set_id=0, index=3
```

The registry stores:
- her **nullifier** (`SHA256(pub_key ‖ "alice")`) — for Sybil resistance
- her **commitment** (`r·G + v·H`) — a secp256k1 point that hides both her
  identity and her blinding key

Alice keeps `blinding` secret.  It is never transmitted.

---

## Step 2 — Bob checks Alice is registered

Bob has Alice's `pub_key` and name.  He calls:

```
POST /api/v1/has
{ "pub_key": "<alice's pub_key>", "name": "alice" }
```

Response:

```json
{ "present": true, "nullifier": "539e8b..." }
```

**What this confirms:** the registry holds a commitment for
`SHA256(alice_pub_key ‖ "alice")`.  Alice is a unique, registered member —
the same `(pub_key, name)` pair cannot be registered twice (Sybil resistance).

Bob saves the `nullifier` for the next step.

---

## Step 3 — Alice proves she controls the identity

Bob now asks Alice to prove she actually owns the commitment — not just that
someone registered it.  This requires her private `blinding` key.

Alice runs locally (takes ~2–5 s in release build):

```bash
veiled prove \
  --pub-key  <pub_key> \
  --name     alice \
  --blinding <blinding> \
  --set-id   0 \
  --index    3
# proof (878 bytes):
# a3f2c1...
```

She sends Bob the 878-byte proof hex.

---

## Step 4 — Bob verifies the proof

Bob submits the proof along with the nullifier he obtained in step 2:

```
POST /api/v1/verify
{
  "nullifier": "539e8b...",
  "set_id": 0,
  "proof": "a3f2c1..."
}
```

Response:

```json
{ "valid": true }
```

**What this confirms:** Alice knows the blinding key `r` for a commitment
in set 0 that opens correctly to her nullifier value.  She is the actual
owner — not someone who merely knows her public details.

**Bob sends the money.**

---

## What each party learns

| Party | Learns | Does NOT learn |
|---|---|---|
| Registry (at registration) | nullifier, commitment | pub_key, blinding, name |
| Bob (step 2) | Alice is registered; her nullifier | her blinding key |
| Bob (step 4) | Alice controls the commitment | which index in the set is hers |
| Eve (observer) | nullifier is public | cannot derive pub_key or blinding from it |

---

## Why Eve cannot impersonate Alice

| Eve's attempt | Result |
|---|---|
| Present Alice's pub_key + name without a proof | Bob demands proof; Eve has no blinding key |
| Generate a fake proof for Alice's nullifier | Requires solving discrete log on secp256k1 — computationally infeasible |
| Register Alice's identity a second time | Registry returns **409 Conflict** — nullifier already used |
| Register a different pub_key as "alice" | Different nullifier — Bob's `/has` check on Alice's pub_key returns `present: false` |

---

## Security concerns

### Spoofing

**Identity spoofing** — Eve claims to be Alice by presenting Alice's
`pub_key` and name.  The `/has` check alone does not stop this: anyone who
knows those two values can make the same request.  This is why Bob must
always proceed to step 4 and demand a ZK proof.  The proof is unforgeable
without `blinding`, which only Alice holds.

**Nullifier spoofing** — Eve intercepts the nullifier from Bob's `/has`
response and tries to submit her own proof against it.  This fails: the
proof must show knowledge of a commitment opening `(v, r)` where
`v = scalar(nullifier)`.  Without the correct `r` (Alice's blinding key)
Eve cannot construct a valid proof — doing so requires solving the discrete
logarithm problem on secp256k1.

**Replay** — Eve records Alice's proof and replays it to Bob for a second
payment.  The proof is tied to a specific `(nullifier, set_id)` pair.  Bob
should treat a proof as single-use by tracking which nullifiers he has
already accepted; the proof itself does not expire cryptographically.

---

### Privacy

**What the registry learns** — At registration the registry receives the
nullifier and commitment.  The nullifier is `SHA256(pub_key ‖ name)`:
one-way, so the registry cannot reverse it to learn `pub_key` or `name`.
The commitment `r·G + v·H` is computationally opaque without `r`.
The registry therefore learns nothing about Alice's identity beyond the
fact that *some* unique entity registered.

**What Bob learns** — Bob receives Alice's `pub_key` and name directly
from Alice (she chose to share them).  Via `/has` he learns her nullifier.
Via `/verify` he learns she controls a commitment in the set — but not
*which* commitment, because the ZK proof hides the index.  Across multiple payments Bob always sees the same nullifier, so he can recognise Alice as
the same person, but only because Alice chose to give him her `pub_key`
and name each time.

**Linkability across verifiers** — If Alice proves membership to both Bob
and Carol, they each see the same nullifier (`SHA256(pub_key ‖ name)`).
If Bob and Carol collude and compare nullifiers they can determine that
the same person paid both of them.  To prevent this Alice would need a
different `(pub_key, name)` pair per relationship — each requiring a
separate registration.

**Blinding key leakage** — If Alice's `blinding` key is ever exposed, her
commitment can be recomputed and linked to her nullifier.  The blinding key
must be kept secret and stored securely (e.g., in the keyfile produced by
`save-key`).

**Registry as a surveillance point** — The registry sees every nullifier
at registration time.  A malicious or compromised registry could attempt
to correlate registration timing, IP addresses, or request metadata with
real identities.  These are operational-security concerns outside the
cryptographic model.

---

### Multi-party unlinkability

By default Alice uses the same `(pub_key, name)` pair with everyone she
pays, so any two verifiers who compare notes can identify her as the same
person.  veiled supports a stronger mode with no code changes.

**The mechanic — one key pair per relationship:**

```bash
# Alice generates a dedicated key pair for her relationship with Bob
veiled generate-key
veiled save-key --pub-key <pk_B> --blinding <bk_B> --out alice-bob.json
veiled register --pub-key <pk_B> --name alice --blinding <bk_B>
# → nullifier_B = SHA256(pk_B ‖ "alice")

# Alice generates a separate key pair for her relationship with John
veiled generate-key
veiled save-key --pub-key <pk_J> --blinding <bk_J> --out alice-john.json
veiled register --pub-key <pk_J> --name alice --blinding <bk_J>
# → nullifier_J = SHA256(pk_J ‖ "alice")
```

`pk_B ≠ pk_J` → `nullifier_B ≠ nullifier_J` — the two nullifiers are
mathematically independent.  The name `"alice"` can repeat freely because
the `pub_key` makes each nullifier unique.

**What each party learns:**

| Party | Learns | Does NOT learn |
|---|---|---|
| Bob | `pk_B`, `nullifier_B` — Alice's identity with him | `pk_J`, `nullifier_J`, or that Alice has other relationships |
| John | `pk_J`, `nullifier_J` — Alice's identity with him | `pk_B`, `nullifier_B`, or that Alice has other relationships |
| Bob + John (colluding) | Their own nullifiers | Cannot link the two — different nullifiers, different public keys |
| Registry | Two independent nullifiers + commitments | Cannot link them to the same person without external information |

**What remains linkable:**

- The registry sees both registrations.  If it logs IP addresses or
  timestamps it may correlate them operationally, even though the
  cryptographic data is unlinkable.
- If Alice ever reveals the connection (e.g. by telling Bob she also
  pays John), the unlinkability is lost — this is a social, not a
  cryptographic, guarantee.

**Rule of thumb:** one `generate-key` call per relationship, one keyfile
per relationship.  The name can be the same everywhere; only the key pair
needs to change.

---

### What veiled does NOT provide

| Property | Status |
|---|---|
| Hiding Alice's identity from Bob | **No** — Alice shares `pub_key` + name with Bob directly |
| Proof expiry / revocation | **No** — a valid proof stays valid until the set changes |
| Forward secrecy of the blinding key | **No** — if `blinding` leaks later, old registrations are deanonymised |
| Protection against a malicious registry | **Partial** — crypto is sound, but timing/metadata leaks are not addressed |
| Cross-session unlinkability (same verifier) | **No** — same nullifier is reused; use a fresh `(pub_key, name)` per relationship for full unlinkability |

---

## Full CLI walkthrough

```bash
# Alice — one-time setup
veiled generate-key
veiled save-key --pub-key <pk> --blinding <bk> --out alice-keys.json
veiled register --pub-key <pk> --name alice --blinding <bk>
# → set_id=0, index=3

# Bob — check registration (has Alice's pub_key and name)
curl -s -X POST http://localhost:7271/api/v1/has \
  -H 'content-type: application/json' \
  -d '{"pub_key":"<pk>","name":"alice"}'
# → {"present":true,"nullifier":"539e8b..."}

# Alice — generate proof on request from Bob
veiled prove --pub-key <pk> --name alice --blinding <bk> --set-id 0 --index 3
# → proof (878 bytes): a3f2c1...

# Bob — verify proof before sending money
curl -s -X POST http://localhost:7271/api/v1/verify \
  -H 'content-type: application/json' \
  -d '{"nullifier":"539e8b...","set_id":0,"proof":"a3f2c1..."}'
# → {"valid":true}

# Bob sends money.
```
