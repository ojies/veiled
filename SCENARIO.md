# Scenario: Bob sends Alice money

This document walks through the practical use of veiled in a peer-to-peer
payment context.  All commands use the CLI; all HTTP calls can be made with
any HTTP client.

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
