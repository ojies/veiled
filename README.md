<p align="center">
  <img src="docs/images/banner.svg" alt="Veiled — Verified Payments, Veiled Identities" width="700"/>
</p>


A Bitcoin pseudonymous payment verification system implementing the
[Anonymous Self-Credentials (ASC)](docs/annomymous-credential.pdf) protocol
by Alupotha et al., with a Common Reference String (CRS) and Bootle/Groth
one-out-of-many proofs on secp256k1.

Veiled allows beneficiaries to receive payments from multiple merchants while
keeping their true identity private, but still allowing merchants to
authenticate beneficiaries before sending payment. A beneficiary registers a
single master credential once, derives an unlinkable payment identity for each
merchant using a hash-based key derivation function, and proves ownership of a
legitimate credential through a zero-knowledge proof without revealing which
credential is theirs among a public anonymity set. Each merchant receives a
unique nullifier that binds the payment identity to the beneficiary's master
credential, preventing the same beneficiary from claiming multiple payment
identities with the same merchant, while each merchant's pseudonym is
cryptographically unlinkable to pseudonyms held by other merchants. Once
registered, the beneficiary authenticates to each merchant using a lightweight
Schnorr signature with no further interaction with the registry required, and
receives Bitcoin payments to a P2TR address derived from their pseudonym.

> **Privacy note:** The `friendly_name` (e.g., "alice") is revealed to each
> merchant during payment identity registration. Colluding merchants could
> match on names to link identities. The cryptographic identifiers (pseudonyms,
> nullifiers) themselves remain unlinkable across merchants.

---

## Protocol overview

The protocol has six phases:

### Phase 0 — System Setup (CRS)

Merchants register with the registry. An admin creates an anonymity set,
which triggers CRS generation from the registered merchants:

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
they are provably independent (NUMS — Nothing Up My Sleeve).

### Phase 1 — Credential Creation (local, offline)

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

### Phase 2 — Registration + Anonymity Set Finalization

The beneficiary registers their commitment Φ with the registry via gRPC, then
subscribes to a server-streaming RPC to wait for the anonymity set to finalize:

```
RegisterBeneficiary(set_id, Φ, name)  →  index
SubscribeSetFinalization(set_id)      →  stream(anonymity_set)
GetVtxoTree(set_id)                   →  (root_tx, fanout_tx)
```

Once the set is finalized, the beneficiary downloads the frozen anonymity set
and VTxO tree. The sealed set is anchored on Bitcoin via a vtxo-tree — a
binary tree of pre-signed transactions where the root UTXO commits to all
leaf outputs. Each leaf is a P2TR output locked to one beneficiary's Φ.

### Phase 3 — Payment Identity Registration

The beneficiary derives a per-merchant child credential and proves membership:

```
csk_l = HKDF(r, salt=merchant_name, info="CRS-ASC-child-secret-key")
ϕ_l   = csk_l · g              # pseudonym (unlinkable across merchants)
nul_l = s_l · g                 # public nullifier (Sybil resistance)
```

A composite zero-knowledge proof demonstrates:
1. **Membership**: "I know the opening of one of the commitments in the
   anonymity set" (adapted Bootle/Groth on shifted commitments)
2. **Nullifier authenticity**: "nul_l = s_l · g is correctly derived from
   my committed identity" (Schnorr proof)

The beneficiary submits `(ϕ_l, nul_l, proof, friendly_name)` to the merchant
via `SubmitPaymentRegistration`.

### Phase 4 — Merchant Verification

The merchant verifies the ZK proof against the CRS and anonymity set,
checks the nullifier for duplicates (Sybil resistance), and stores the
mapping `pseudonym → (friendly_name, nullifier, set_id)`.

### Phase 5 — Payment Request

The beneficiary authenticates via a non-interactive Schnorr proof of child
credential knowledge:

```
t ←$ Z_q,  R = t · g
e = H("CRS-ASC-schnorr-child-auth" || g || ϕ || R)
s = t + e · csk_l
Proof = (R, s)
```

Submitted via `SubmitPaymentRequest(amount, pseudonym, proof)`. The merchant
verifies the proof, looks up the pseudonym, and returns a P2TR Bitcoin address
derived from the pseudonym.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  Registry gRPC Server (veiled-registry-grpc)                        │
│                                                                     │
│  RegistryStore (in-memory)                                          │
│  ├── merchant_pool: HashMap<String, MerchantInfo>                   │
│  └── active_sets: HashMap<u64, ActiveSet>                           │
│       ├── registry: Registry (CRS + anonymity set)                  │
│       ├── tree: Option<IdentityTree> (VTxO tree after finalization) │
│       └── finalization_tx: watch::Sender<bool> (notification)       │
│                                                                     │
│  gRPC RPCs:                                                         │
│  ├── RegisterMerchant, CreateSet, RegisterBeneficiary, FinalizeSet  │
│  ├── GetMerchants, GetCrs, GetAnonymitySet, GetVtxoTree             │
│  └── SubscribeSetFinalization (server-streaming)                    │
└───────────────────┬────────────────────────┬────────────────────────┘
                    │ gRPC                   │ gRPC
                    ▼                        ▼
┌──────────────────────────────┐  ┌───────────────────────────────────┐
│  Merchant gRPC Server        │  │  Beneficiary CLI                  │
│                              │  │                                   │
│  MerchantGrpcService         │  │  1. Fetch CRS from registry       │
│  ├── merchant: Merchant      │  │  2. Create credential locally     │
│  ├── crs: Crs                │  │  3. Register Φ with registry      │
│  └── anonymity_set           │  │  4. Subscribe to finalization     │
│                              │  │  5. Fetch VTxO tree               │
│  gRPC RPCs:                  │  │  6. Register payment identity     │
│  ├── SubmitPaymentRegistration│  │     with merchant (Phase 3-4)    │
│  └── SubmitPaymentRequest    │  │  7. Submit payment request        │
│                              │  │     to merchant (Phase 5)         │
└──────────────────────────────┘  └───────────────────────────────────┘
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [SCENARIO.md](docs/SCENARIO.md) | End-to-end walkthrough (Alice, CoffeeCo, BookStore, TechMart) |
| [API.md](docs/API.md) | gRPC API reference (Registry + Merchant services) |
| [CRYPTOGRAPHY.md](docs/CRYPTOGRAPHY.md) | Cryptographic primitives and terminology |
| [LAYOUT.md](docs/LAYOUT.md) | Project directory structure |
| [ASC paper](docs/annomymous-credential.pdf) | Original protocol by Alupotha et al. |

---

## Running

### Start the registry

```bash
cargo run --bin veiled-registry-grpc
# INFO: Veiled gRPC Registry listening on [::1]:50051
```

Options: `--listen <addr>` (default: `[::1]:50051`)

### Start a merchant

```bash
cargo run --bin merchant -- \
  --name "Merchant1" \
  --origin "https://merchant1.com" \
  --set-id 1 \
  --listen "[::1]:50061"
```

The merchant registers with the registry, fetches the CRS, and subscribes
to set finalization. Once finalized, it starts accepting beneficiary
connections.

### Run a beneficiary

```bash
# Register credential and payment identity (Phases 1-4)
cargo run --bin beneficiary -- \
  --name "alice" \
  --set-id 1 \
  --merchant-server "http://[::1]:50061" \
  --merchant-id 1

# Full flow including payment request (Phases 1-5)
cargo run --bin beneficiary -- \
  --name "alice" \
  --set-id 1 \
  --merchant-server "http://[::1]:50061" \
  --merchant-id 1 \
  --payment-amount 5000
```

Options: `--merchant-name <name>` overrides registry lookup for merchant name.

### Run the demo

A self-contained simulation that starts an in-process registry, 3 merchant
servers, and runs 8 beneficiaries through the full Phase 0-5 protocol:

```bash
cargo run --bin demo --release
```

Demonstrates: credential creation, registration, set finalization, ZK proof
generation/verification, Schnorr authentication, P2TR address derivation,
and cross-merchant pseudonym unlinkability.

---

## Testing

```bash
cargo test                      # all tests
cargo test -- --skip proof      # fast: skip slow proof tests
```

Test coverage:
- **core**: CRS generator independence; Pedersen commitment properties;
  HKDF nullifier derivation; credential creation; beneficiary/merchant
  lifecycle; ZK proof generation/verification; payment request Schnorr
  proof; full Phase 0-5 flow test
- **registry gRPC**: merchant registration + duplicate rejection; set
  creation + CRS generation; beneficiary registration + capacity enforcement;
  set finalization + VTxO tree; streaming subscription (before/after
  finalization); error cases (unknown set, duplicate beneficiary)

---

## Roadmap

- [x] CRS generation with HashToCurve generators (Phase 0)
- [x] Multi-value Pedersen commitments (Phase 0)
- [x] HKDF per-merchant nullifier derivation (Phase 0)
- [x] MasterCredential creation (Phase 1)
- [x] Beneficiary registration + anonymity set finalization (Phase 2)
- [x] Bitcoin anchoring via VTxO tree (Phase 2)
- [x] Server-streaming subscription for set finalization (Phase 2)
- [x] Payment identity registration with ZK proof (Phase 3)
- [x] Merchant verification of ZK proofs (Phase 4)
- [x] Payment request with Schnorr authentication (Phase 5)
- [x] P2TR address derivation from pseudonyms (Phase 5)
- [x] gRPC services for registry and merchant
- [x] Beneficiary CLI with full Phase 1-5 flow

### Phase 3-4 improvements (Payment Identity Registration)
- [ ] Per-merchant nullifier duplicate rejection in the merchant service (currently verified in core but not enforced at the gRPC layer)
- [ ] Persistent storage for merchant registered identities (survives restarts)
- [ ] Privacy-preserving name binding — replace plaintext `friendly_name` with a commitment or blinded identifier to prevent cross-merchant linkage via name matching
- [ ] Batch payment identity registration (register with multiple merchants in a single flow)

### Phase 5 improvements (Payment Request)
- [ ] Payment amount limits and rate limiting per pseudonym
- [ ] Payment receipt / proof-of-payment from merchant back to beneficiary
- [ ] Multi-payment support — multiple sequential payments under the same pseudonym without re-registration
- [ ] On-chain P2TR spending path — enable beneficiaries to claim VTxO tree leaf outputs using their credential

### Infrastructure
- [ ] Persistent storage for registry state (SQLite or similar)
- [ ] TLS for gRPC connections
- [ ] Credential revocation / proof expiry
- [ ] Anonymity set size scaling beyond current capacity
