# Protocol Overview

Veiled implements the Anonymous Self-Credentials (ASC) construction by
Alupotha et al. on Bitcoin's secp256k1 curve, using Bootle/Groth
one-out-of-many proofs for anonymous set membership and Schnorr signatures
for lightweight authentication.

---

## Participants

| Role | Description |
|------|-------------|
| **Registry** | Coordinates merchant registration, manages anonymity sets, generates the CRS, and builds VTxO trees anchored on Bitcoin |
| **Merchant** | Payment provider that verifies ZK proofs, stores pseudonymous identities, and sends Bitcoin payments |
| **Beneficiary** | End user who creates a master credential, registers anonymously, and receives payments under unlinkable pseudonyms |

---

## Phase 0 — System Setup (CRS)

Merchants register with the registry. An anonymity set is created, which
triggers Common Reference String (CRS) generation from the registered
merchants:

```
crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)
```

- **G** = secp256k1, **q** = curve order
- **g** = HashToCurve("CRS-ASC-generator-0") — base generator (NUMS)
- **h_l** = HashToCurve("CRS-ASC-generator-{l}") for l = 1..L — per-merchant generators
- **h_name** = HashToCurve("CRS-ASC-generator-name") — name commitment generator
- **v_l** = merchant name (string identifier, used as HKDF salt)
- **G_auth_l** = HashToCurve(merchant_name) — credential generator for merchant l

All generators are derived via hash-to-curve with DST `"CRS-ASC-v1"`, ensuring
they are provably independent (NUMS — Nothing Up My Sleeve). See
[CRYPTOGRAPHY.md](CRYPTOGRAPHY.md) for details on generator independence.

### Registry synchronization

The CRS is bound to the merchant set that exists when the anonymity set is
created. Beneficiaries in set T₀ can only interact with merchants registered
at T₀. If new merchants register at T₁, only beneficiaries in the T₁ set
(or later) can access them. This is an inherent property of the batch-based
design — the cryptographic proof references a static registry snapshot.

---

## Phase 1 — Credential Creation (local, offline)

The beneficiary generates three secrets locally and computes a master identity:

```
sk ←$ {0,1}^256     # root secret for nullifier derivation (MasterSecret)
r  ←$ {0,1}^256     # child credential randomness (ChildRandomness)
k  ←$ Z_q           # Pedersen blinding key (BlindingKey)

for l = 1..L:
  s_l = HKDF(sk, salt=v_l, info="CRS-ASC-nullifier")

name_scalar = SHA256(friendly_name)

Φ = k·g + s_1·h_1 + ... + s_L·h_L + name_scalar·h_name

Master credential = (Φ, sk, r, k)
```

Three independent secrets ensure no information leakage between protocol
layers. `sk` derives all nullifiers, `r` derives all per-merchant
authentication keys, and `k` blinds the commitment. The beneficiary stores
only `(sk, r, k)` — about 96 bytes. Φ can always be recomputed from these
secrets plus the public CRS.

---

## Phase 2 — Registration and Anonymity Set Finalization

The beneficiary pays a registration fee to the registry's P2TR address, then
registers their commitment Φ along with the payment outpoint. The registry
verifies the on-chain payment before admitting the beneficiary:

```
GetFees()                             →  beneficiary_fee, merchant_fee
GetRegistryAddress(set_id)            →  address, internal_key
<pay beneficiary_fee to registry address>
RegisterBeneficiary(set_id, Φ, name, funding_txid, funding_vout)  →  index
SubscribeSetFinalization(set_id)      →  stream(anonymity_set)
GetVtxoTree(set_id)                   →  (root_tx, fanout_tx)
```

The registry fetches the referenced transaction via `getrawtransaction`,
verifies the output at `funding_vout` pays the correct P2TR address with at
least `beneficiary_fee` sats, then adds Φ to the anonymity set.

Once the set reaches its capacity, it is **finalized** — sealed permanently
with no further additions or removals. The registry signs both transactions
with the aggregate key and broadcasts them. It then builds a **VTxO tree**:

```
            [Root TX]  ← single UTXO broadcast on Bitcoin
           /          \
     [Fanout TX]      ...
     /    |    \
  [Φ_1] [Φ_2] ... [Φ_N]   ← N P2TR outputs, one per beneficiary
```

Each leaf is a P2TR output whose internal key is the beneficiary's Φ. This
works because Φ is already a valid compressed secp256k1 public key. All
subscribers are notified via the streaming RPC. The beneficiary downloads
the frozen anonymity set and VTxO tree — both are needed for proof
generation.

### Funding and broadcast

Beneficiaries and merchants pay registration fees to the registry's P2TR
address (queried via `GetRegistryAddress`). Fee amounts are configured on the
registry and queried via `GetFees`. The registry verifies each payment
on-chain before admitting participants.

The collected fees fund the VTxO tree. At finalization, the funding UTXO is
sent to the **aggregate address** (derived from all beneficiary pubkeys via
`GetAggregateAddress`). The registry signs both `root_tx` and `fanout_tx`
with the aggregate secret key and broadcasts them to the Bitcoin network.

---

## Phase 3 — Payment Identity Registration

The beneficiary derives a per-merchant child credential and proves membership
in the anonymity set:

```
csk_l = HKDF(r, salt=merchant_name, info="CRS-ASC-child-secret-key")
ϕ_l   = csk_l · g              # pseudonym (unlinkable across merchants)
nul_l = s_l · g                 # public nullifier (Sybil resistance)
```

A composite zero-knowledge proof demonstrates two things simultaneously:

1. **Membership**: "I know the opening of one of the commitments in the
   anonymity set" — adapted Bootle/Groth proof on shifted commitments
   `D[i] = Φ_i - s_l·h_l`. At the prover's index, the merchant's term
   cancels, reducing to a commitment-to-zero problem.

2. **Nullifier authenticity**: "nul_l = s_l · g is correctly derived from
   my committed identity" — Schnorr proof binding the nullifier to the
   commitment.

The proof also embeds `name_scalar = SHA256(friendly_name)`, which the
merchant can verify to confirm the beneficiary's friendly name.

The beneficiary submits `(ϕ_l, nul_l, proof, friendly_name)` to the merchant
via `SubmitPaymentRegistration`. See [CRYPTOGRAPHY.md](CRYPTOGRAPHY.md) for
the full proof structure and size analysis.

---

## Phase 4 — Merchant Verification

The merchant verifies the ZK proof against the CRS and anonymity set:

1. Reconstructs shifted commitments `D[i] = Φ_i - nul_l·h_l` for all i
2. Verifies the Bootle/Groth membership proof over the shifted set
3. Verifies the Schnorr proof that `nul_l = s_l · g`
4. Verifies `name_scalar` matches `SHA256(friendly_name)`
5. Checks nullifier freshness — rejects if `nul_l` was already seen (Sybil
   resistance)
6. Stores the mapping: `pseudonym → (friendly_name, nullifier, set_id)`

---

## Phase 5 — Payment Request

The beneficiary authenticates via a non-interactive Schnorr proof of child
credential knowledge:

```
t ←$ Z_q,  R = t · g
e = H("CRS-ASC-schnorr-child-auth" || g || ϕ || R)
s = t + e · csk_l
Proof = (R, s)
```

Submitted via `SubmitPaymentRequest(amount, pseudonym, proof)`. The merchant:
1. Verifies the Schnorr proof: checks `s·g == R + e·ϕ_l`
2. Looks up the pseudonym in registered identities
3. Derives a P2TR Bitcoin address from the beneficiary's pseudonym
4. Sends the requested Bitcoin amount to the beneficiary's P2TR address

After the initial registration (Phases 3-4), the beneficiary can authenticate
indefinitely using these lightweight Schnorr proofs — no further interaction
with the registry is required.

---

## Cross-Merchant Unlinkability

When a beneficiary registers with multiple merchants, they derive completely
different cryptographic identifiers for each:

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
- The ZK proofs reveal nothing about which commitment in the set is the
  beneficiary's

**Privacy note:** The `friendly_name` is revealed to each merchant during
registration. Colluding merchants could match on names. The cryptographic
identifiers remain unlinkable.

---

## Information Disclosure

| Party | Learns | Does NOT learn |
|-------|--------|----------------|
| **Registry** | Φ (commitment), beneficiary name | sk, r, k, which merchant the beneficiary will use |
| **Bitcoin** (on-chain) | Φ as a P2TR leaf key in VTxO tree | Nothing about the commitment's contents |
| **Merchant A** | ϕ_A (pseudonym), nul_A, friendly_name | Beneficiary's index in the set, blinding key k, Merchant B's identifiers |
| **Two colluding merchants** | Both see the friendly_name — can match | Cannot link pseudonyms or nullifiers cryptographically |

---

## Sybil Resistance

Operates at two levels:

**Per merchant**: Each merchant stores seen public nullifiers. The same `sk` +
same merchant always produces the same `nul_l`, so a beneficiary cannot
register two payment identities with the same merchant.

**Per anonymity set**: The commitment Φ is unique per beneficiary (different
secrets produce different curve points). The registry rejects duplicate Φ
values within a set.

---

## Security Properties

| Attack | Result |
|--------|--------|
| Register with someone else's sk | Same Φ — registry rejects duplicate commitment |
| Forge a proof for another's Φ | Requires knowing k and all s_l — discrete log is infeasible |
| Replay a CoffeeCo proof to BookStore | Different service_index in Fiat-Shamir challenge — proof invalid |
| Replay a payment request | Same pseudonym verified, but only the original address is returned |
| Merchant tries to determine beneficiary's index | ZK proof reveals nothing about which commitment is theirs |
| Two merchants compare nullifiers | nul_1 ≠ nul_2 — HKDF makes them independent |
| Two merchants compare names | Both see the friendly_name — linkable by design |
