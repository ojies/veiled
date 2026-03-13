# Scenario: Pseudonymous Payment Registration on Bitcoin

This document walks through the Veiled protocol end-to-end, showing how a
beneficiary creates a master credential, registers with a registry backed by
Bitcoin, and establishes pseudonymous payment identities with merchants — all
while maintaining zero-knowledge membership privacy.

For the protocol specification, see [PROTOCOL.md](PROTOCOL.md). For
cryptographic details, see [CRYPTOGRAPHY.md](CRYPTOGRAPHY.md).

---

## Cast

- **Registry**: A gRPC server that manages merchants, anonymity sets, CRS
  generation, and VTxO tree construction. Stateful but does not participate
  in the cryptographic protocol after setup.
- **Merchant 1** ("CoffeeCo"): A payment provider that verifies beneficiary
  proofs and sends Bitcoin payments.
- **Merchant 2** ("BookStore"): Another payment provider. Alice will register
  with both to demonstrate cross-merchant unlinkability.
- **Alice**: A beneficiary who wants to receive payments from both merchants
  without them being able to link her pseudonyms.

---

## Phase 0 — Registry and CRS Setup

An admin starts the registry and registers the merchants:

```
# Start registry
veiled-registry-grpc --listen [::1]:50051

# Register merchants (via gRPC client)
RegisterMerchant { name: "CoffeeCo", origin: "https://coffeeco.com" }
RegisterMerchant { name: "BookStore", origin: "https://bookstore.com" }

# Create anonymity set for 8 beneficiaries with both merchants
CreateSet { set_id: 1, merchant_names: ["CoffeeCo", "BookStore"], beneficiary_capacity: 8 }
```

When `CreateSet` is called, the registry:
1. Looks up CoffeeCo and BookStore in its merchant pool
2. Builds a **CRS** from their names using hash-to-curve:

```
g      = HashToCurve("CRS-ASC-generator-0",     DST="CRS-ASC-v1")  # base generator
h_1    = HashToCurve("CRS-ASC-generator-1",     DST="CRS-ASC-v1")  # CoffeeCo generator
h_2    = HashToCurve("CRS-ASC-generator-2",     DST="CRS-ASC-v1")  # BookStore generator
h_name = HashToCurve("CRS-ASC-generator-name",  DST="CRS-ASC-v1")  # name generator
```

All generators are provably independent (NUMS — nobody knows the discrete log
relationships between them). The CRS is public — beneficiaries and merchants
fetch it from the registry.

---

## Phase 1 — Alice Creates Her Credential (local, offline)

Alice fetches the CRS from the registry via `GetCrs(set_id: 1)`, then creates
her credential locally. Nothing touches the network in this step.

### Step 1: Generate three secrets

```
sk ←$ {0,1}^256     # master secret — derives all nullifiers
r  ←$ {0,1}^256     # child randomness — derives per-merchant auth keys
k  ←$ Z_q           # blinding key — hides everything in the commitment
```

Three independent secrets ensure no information leakage between protocol
layers. If `sk` were reused for blinding, learning the blinding factor would
reveal nullifier information.

### Step 2: Derive per-merchant nullifier scalars

```
s_1 = HKDF(sk, salt="CoffeeCo",  info="CRS-ASC-nullifier")
s_2 = HKDF(sk, salt="BookStore", info="CRS-ASC-nullifier")
```

HKDF guarantees the two outputs are computationally indistinguishable from
independent random values, even to someone who knows both merchant names.

### Step 3: Compute the master identity commitment

```
name_scalar = SHA256("alice")

Φ = k·g + s_1·h_1 + s_2·h_2 + name_scalar·h_name
```

This packs both nullifier scalars and Alice's name hash into a single 33-byte
secp256k1 point. The blinding key `k` hides everything.

**Result**: Alice's master credential is `(Φ, sk, r, k)`. She stores
`(sk, r, k)` — about 96 bytes. Φ can always be recomputed from these secrets
plus the public CRS.

---

## Phase 2 — Alice Registers and Waits for Finalization

### Step 1: Register Φ with the registry

```
RegisterBeneficiary {
    set_id: 1,
    phi: Φ,            # 33-byte commitment
    name: "alice"
}
→ BeneficiaryResponse { index: 0 }
```

The registry appends Φ to the anonymity set for set 1 and returns Alice's
index. The set has capacity 8 — once 8 beneficiaries register, the set can
be finalized.

### Step 2: Subscribe to finalization

```
SubscribeSetFinalization { set_id: 1 }
→ stream waiting...
```

Alice opens a server-streaming gRPC connection. This blocks until the set is
finalized. She doesn't need to poll — the registry notifies all subscribers
via a `tokio::sync::watch` channel when finalization occurs.

Meanwhile, 7 other beneficiaries (Bob, Carol, Dave, Eve, Frank, Grace, Heidi)
register their own commitments Φ_2 through Φ_8.

### Step 3: Set finalization

An admin finalizes the set once all 8 beneficiaries have registered:

```
FinalizeSet {
    set_id: 1,
    sats_per_user: 10_000,
    funding_txid: <32-byte txid>,
    funding_vout: 0
}
```

The registry:
1. **Seals** the anonymity set (frozen permanently — no additions or removals)
2. Builds a **VTxO tree** from all 8 commitments:

```
            [Root TX]  ← single UTXO broadcast on Bitcoin
           /          \
     [Fanout TX]      ...
     /    |    \
  [Φ_1] [Φ_2] ... [Φ_8]   ← 8 P2TR outputs, one per beneficiary
```

Each leaf is a P2TR output whose internal key is the beneficiary's Φ. This
works because Φ is already a valid compressed secp256k1 public key.

3. Notifies all subscribers via the streaming RPC

### Step 4: Alice receives the finalized set

Alice's stream resolves with the complete anonymity set:

```
GetAnonymitySetResponse {
    commitments: [Φ_1, Φ_2, ..., Φ_8],
    finalized: true,
    count: 8,
    capacity: 8
}
```

She registers locally, determining her index (0) in the set. She also fetches
the VTxO tree:

```
GetVtxoTree { set_id: 1 }
→ GetVtxoTreeResponse { root_tx: <bytes>, fanout_tx: <bytes> }
```

Alice needs the full anonymity set stored locally because the Bootle/Groth ZK
proof requires the prover to have the entire ring of commitments.

---

## What the anonymity set looks like

```
Λ = [Φ_1 (Alice), Φ_2 (Bob), ..., Φ_8 (Heidi)]

Each Φ_i = k_i·g + s_{i,1}·h_1 + s_{i,2}·h_2 + name_i·h_name
```

To an observer, the set is 8 random-looking 33-byte curve points. There is no
way to tell which belongs to Alice without knowing her secrets.

---

## Phase 3 — Alice Registers Payment Identity with CoffeeCo

Alice wants to receive payments from CoffeeCo (merchant 1). She derives a
per-merchant identity and proves she's a legitimate member of the anonymity set.

### Step 1: Derive child credential

```
csk_1 = HKDF(r, salt="CoffeeCo", info="CRS-ASC-child-secret-key")
ϕ_1   = csk_1 · g     # pseudonym — Alice's public identity at CoffeeCo
```

### Step 2: Derive public nullifier

```
nul_1 = s_1 · g       # public nullifier — Sybil resistance at CoffeeCo
```

If Alice tries to register again with CoffeeCo, she'll produce the same
`nul_1`, and the merchant will reject the duplicate.

### Step 3: Generate zero-knowledge proof

The proof demonstrates two things simultaneously:

1. **Membership**: "I know the opening of one of the 8 commitments in Λ,
   without revealing which one."

   The proof works by **shifting** each commitment:
   ```
   D[i] = Φ_i - s_1·h_1    for all i = 1..8
   ```
   At Alice's index (0), this cancels the CoffeeCo term:
   ```
   D[0] = k·g + s_2·h_2 + name_scalar·h_name
   ```
   An adapted Bootle/Groth proof then proves knowledge of the opening to one
   of the 8 shifted commitments.

2. **Nullifier authenticity**: "nul_1 = s_1 · g is correctly derived from my
   committed identity." (Schnorr proof)

The proof also embeds `name_scalar = SHA256("alice")`, which the merchant can
verify to confirm Alice's friendly name.

### Step 4: Submit to merchant

```
SubmitPaymentRegistration {
    pseudonym: ϕ_1,          # 33 bytes
    public_nullifier: nul_1, # 33 bytes
    set_id: 1,
    service_index: 1,
    friendly_name: "alice",
    proof: <serialized proof>
}
→ "Payment identity 'alice' registered successfully"
```

---

## Phase 4 — CoffeeCo Verifies the Registration

CoffeeCo's merchant server receives the registration and:

1. Deserializes the zero-knowledge proof
2. Calls `receive_payment_registration(&crs, &anonymity_set, &registration)`:
   - Reconstructs the shifted commitments `D[i] = Φ_i - nul_1·h_1`
   - Verifies the Bootle/Groth membership proof over the shifted set
   - Verifies the Schnorr proof that `nul_1 = s_1 · g`
   - Verifies `name_scalar` matches `SHA256("alice")`
3. Checks that `ϕ_1` (pseudonym) is not already registered (duplicate check)
4. Stores the mapping: `ϕ_1 → { friendly_name: "alice", nullifier: nul_1, set_id: 1 }`

**If Alice tries to register again**: the same `nul_1` would be produced
(deterministic from `sk` + "CoffeeCo"), and the merchant would detect the
duplicate pseudonym.

---

## Phase 5 — Alice Requests a Payment

Later, Alice wants to receive a 5000 sat payment from CoffeeCo. She
authenticates with a lightweight Schnorr proof — no ZK proof needed this time.

### Step 1: Create Schnorr proof

```
t ←$ Z_q
R = t · g
e = SHA256("CRS-ASC-schnorr-child-auth" || g || ϕ_1 || R)
s = t + e · csk_1

Proof = (R, s)
```

### Step 2: Submit payment request

```
SubmitPaymentRequest {
    amount: 5000,
    pseudonym: ϕ_1,     # 33 bytes
    proof_r: R,          # 33 bytes (nonce commitment)
    proof_s: s           # 32 bytes (Schnorr response)
}
```

### Step 3: Merchant processes the request

CoffeeCo:
1. Verifies the Schnorr proof: checks `s·g == R + e·ϕ_1`
2. Looks up `ϕ_1` in `registered_identities` → finds "alice"
3. Derives a P2TR (Pay-to-Taproot) Bitcoin address from the pseudonym
4. Sends 5000 sats to Alice's P2TR address and returns:

```
PaymentRequestResponse {
    address: "bc1p...",       # P2TR Bitcoin address
    friendly_name: "alice"
}
```

Alice receives the payment at her P2TR address. The address is derived
deterministically from her pseudonym, so it's consistent across requests
to the same merchant.

---

## Cross-Merchant Unlinkability

If Alice also registers with BookStore (merchant 2), she derives completely
different cryptographic identifiers:

```
# CoffeeCo (merchant 1):
csk_1 = HKDF(r, "CoffeeCo")  →  ϕ_1 = csk_1 · g
s_1   = HKDF(sk, "CoffeeCo") →  nul_1 = s_1 · g

# BookStore (merchant 2):
csk_2 = HKDF(r, "BookStore")  →  ϕ_2 = csk_2 · g
s_2   = HKDF(sk, "BookStore") →  nul_2 = s_2 · g
```

HKDF's pseudorandomness guarantee means:
- `ϕ_1` and `ϕ_2` are computationally independent — cannot be linked
- `nul_1` and `nul_2` are computationally independent — cannot be linked
- The ZK proofs reveal nothing about Alice's index in the anonymity set

**However**: the `friendly_name` "alice" is revealed to both merchants during
Phase 3. If CoffeeCo and BookStore collude and compare their registration
tables, they can match on the name. The cryptographic identifiers (pseudonyms,
nullifiers) remain unlinkable, but the name field breaks full anonymity across
merchants. This is a design trade-off — merchants need to know who they're
paying.

---

## What each party learns

| Party | Learns | Does NOT learn |
|-------|--------|----------------|
| **Registry** | Φ (commitment), beneficiary name | sk, r, k, which merchant Alice will use |
| **Bitcoin** (on-chain) | Φ as a P2TR leaf key in VTxO tree | Nothing about the commitment's contents |
| **CoffeeCo** | ϕ_1 (pseudonym), nul_1, "alice" | Alice's index in the set, blinding key k, BookStore's identifiers |
| **BookStore** | ϕ_2 (pseudonym), nul_2, "alice" | Alice's index in the set, blinding key k, CoffeeCo's identifiers |
| **Two colluding merchants** | Both see "alice" — can match names | Cannot link ϕ_1 to ϕ_2 or nul_1 to nul_2 cryptographically |

---

## Sybil Resistance

Sybil resistance operates at two levels:

### Per merchant
Each merchant stores seen public nullifiers. The same `sk` + same merchant
always produces the same `nul_l`, so a beneficiary cannot register two payment
identities with the same merchant.

### Per anonymity set
The commitment Φ is unique per beneficiary (different secrets produce different
curve points). The registry rejects duplicate Φ values within a set.

---

## Security Properties

| Attack | Result |
|--------|--------|
| Eve registers with Alice's sk | Same Φ — registry rejects duplicate commitment |
| Eve forges a proof for Alice's Φ | Requires knowing k and all s_l — discrete log is infeasible |
| Eve replays Alice's CoffeeCo proof to BookStore | Different service_index in Fiat-Shamir challenge — proof invalid |
| Eve replays Alice's payment request | Same pseudonym verified, but only Alice's address is returned |
| CoffeeCo tries to determine Alice's index | ZK proof reveals nothing about which of the 8 commitments is hers |
| CoffeeCo + BookStore compare nullifiers | nul_1 ≠ nul_2 — HKDF makes them independent |
| CoffeeCo + BookStore compare names | Both see "alice" — names are linkable by design |

---

## Summary of the Flow

```
Phase 0:  Admin registers merchants → creates set → CRS generated
Phase 1:  Alice generates (sk, r, k) → computes Φ locally
Phase 2:  Alice registers Φ → subscribes to stream → set finalized → VTxO tree
Phase 3:  Alice derives (ϕ_1, nul_1) → generates ZK proof → submits to CoffeeCo
Phase 4:  CoffeeCo verifies proof → stores identity → ready for payments
Phase 5:  Alice creates Schnorr proof → CoffeeCo sends payment to P2TR address
```

After Phase 3, Alice can authenticate to CoffeeCo indefinitely using
lightweight Schnorr proofs (Phase 5) — no further interaction with the
registry is required.

---

## Running the Demo

### CLI demo

To see all phases in action with 3 merchants (CoffeeCo, BookStore, TechMart)
and 8 beneficiaries (Alice through Heidi):

```bash
cargo run --bin demo --release
```

The demo starts an in-process registry and merchant servers, runs all 8
beneficiaries through Phases 1-5, and demonstrates cross-merchant pseudonym
unlinkability with Alice registered at all three merchants.

### Interactive web UI (Docker)

The full stack — bitcoind, block explorer, registry, and web UI — runs via
Docker Compose:

```bash
docker compose up --build
# Open http://localhost:3000
```

Create merchants through the UI, then step through the beneficiary flow
interactively with real Bitcoin regtest transactions. See the root
[README.md](../README.md) for details.
