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

## How it works

The protocol has six phases — see [PROTOCOL.md](docs/PROTOCOL.md) for the
full specification:

1. **System Setup** — Merchants register with a registry; a Common Reference
   String (CRS) is generated from NUMS hash-to-curve generators on secp256k1
2. **Credential Creation** — Beneficiary generates secrets locally, computes a
   multi-value Pedersen commitment Φ packing per-merchant nullifiers
3. **Registration** — Φ is registered in an anonymity set, which is sealed and
   anchored on Bitcoin via a Taproot commitment
4. **Payment Identity** — Beneficiary derives an unlinkable pseudonym per
   merchant and proves set membership via a Bootle/Groth ZK proof
5. **Merchant Verification** — Merchant verifies the proof, checks nullifier
   freshness for Sybil resistance, stores the pseudonymous identity
6. **Payment Request** — Beneficiary authenticates via lightweight Schnorr
   proof and provides a P2TR address; merchant sends Bitcoin payment to it

---

## Architecture

```
┌── Docker Compose ──────────────────────────────────────────────────────┐
│                                                                        │
│  ┌─ Web UI (:3000) ─────────────────────────────────────────────────┐ │
│  │  Landing: "I am a Beneficiary" / "I am a Merchant"                │ │
│  │  API routes → gRPC + veiled-core + veiled-wallet                │ │
│  └────────────┬──────────────┬──────────────┬────────────────────────┘ │
│               │ gRPC         │ child_process │ child_process           │
│               ▼              ▼               ▼                         │
│  ┌─────────────────┐  ┌───────────┐  ┌──────────────┐                 │
│  │ Registry gRPC   │  │ veiled-   │  │ veiled-      │                 │
│  │ :50051          │  │ helper    │  │ wallet       │                 │
│  │                 │  │ (crypto)  │  │ (BDK/BIP86)  │                 │
│  │ Merchant pool   │  └───────────┘  └──────┬───────┘                 │
│  │ Anonymity sets  │                        │ RPC                     │
│  │ CRS + Taproot   │                        ▼                         │
│  └────────┬────────┘             ┌──────────────────┐                 │
│           │ gRPC                 │ bitcoind          │                 │
│           ▼                      │ (regtest :18443)  │                 │
│  ┌──────────────────┐            └──────────────────┘                 │
│  │ Merchant gRPC    │            ┌──────────────────┐                 │
│  │ (spawned per     │            │ Block Explorer    │                 │
│  │  merchant)       │            │ (:3002)           │                 │
│  └──────────────────┘            └──────────────────┘                 │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [PROTOCOL.md](docs/PROTOCOL.md) | Protocol specification (6 phases, security properties, unlinkability) |
| [SCENARIO.md](docs/SCENARIO.md) | End-to-end walkthrough (Alice, CoffeeCo, BookStore) |
| [CRYPTOGRAPHY.md](docs/CRYPTOGRAPHY.md) | Cryptographic primitives, Bootle/Groth proof structure, terminology |
| [API.md](docs/API.md) | gRPC + REST API + wallet CLI reference |
| [LAYOUT.md](docs/LAYOUT.md) | Project directory structure |
| [ASC paper](docs/annomymous-credential.pdf) | Original protocol by Alupotha et al. |

---

## Quick start (Docker Compose)

The easiest way to run the full stack — bitcoind, block explorer, registry,
and web UI — all in containers:

```bash
docker compose up --build
```

Services:

| Service | Port | Description |
|---------|------|-------------|
| Web UI | [localhost:3000](http://localhost:3000) | Interactive beneficiary/merchant flows |
| Block Explorer | [localhost:3002](http://localhost:3002) | Bitcoin regtest block explorer |
| Registry gRPC | localhost:50051 | Registry server |
| bitcoind | localhost:18443 | Bitcoin Core regtest node |

A chain-init container automatically creates a miner wallet, mines initial
blocks, and funds the registry. Merchants are created dynamically through
the UI.

Works with Docker or Podman (images use `docker.io/` prefix).

### Running natively

<details>
<summary>Without Docker — requires Rust, Node.js, and bitcoind in PATH</summary>

#### Start the registry

```bash
cargo run --bin veiled-registry-grpc
# INFO: Opening database at registry.db
# INFO: Wallet address: bcrt1p...
# INFO: Veiled gRPC Registry listening on [::1]:50051
```

#### Start a merchant

```bash
cargo run --bin merchant -- \
  --name "Merchant1" \
  --origin "https://merchant1.com" \
  --set-id 1 \
  --listen "[::1]:50061" \
  --funding-txid <64-hex-char-txid> \
  --funding-vout 0
```

#### Run a beneficiary

```bash
cargo run --bin beneficiary -- \
  --name "alice" \
  --set-id 1 \
  --funding-txid <64-hex-char-txid> \
  --funding-vout 0 \
  --merchant-server "http://[::1]:50061" \
  --merchant-id 1 \
  --payment-amount 5000
```

#### Run the simulation

Self-contained simulation with 3 merchants and 8 beneficiaries:

```bash
cargo run --bin simulation --release
```

#### Run the web UI

```bash
./scripts/dev.sh
# Open http://localhost:3000
```

</details>

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
  set finalization + Taproot commitment; streaming subscription (before/after
  finalization); error cases (unknown set, duplicate beneficiary)

---

## Roadmap

- [x] CRS generation with HashToCurve generators (Phase 0)
- [x] Multi-value Pedersen commitments (Phase 0)
- [x] HKDF per-merchant nullifier derivation (Phase 0)
- [x] MasterCredential creation (Phase 1)
- [x] Beneficiary registration + anonymity set finalization (Phase 2)
- [x] Bitcoin anchoring via Taproot commitment (Phase 2)
- [x] Server-streaming subscription for set finalization (Phase 2)
- [x] Payment identity registration with ZK proof (Phase 3)
- [x] Merchant verification of ZK proofs (Phase 4)
- [x] Payment request with Schnorr authentication (Phase 5)
- [x] P2TR address derivation from pseudonyms (Phase 5)
- [x] gRPC services for registry and merchant
- [x] Beneficiary CLI with full Phase 1-5 flow
- [x] Interactive web UI with role-based Beneficiary/Merchant flows
- [x] `veiled-core` CLI bridge for Rust crypto operations from the web UI
- [x] `veiled-wallet` BDK-based wallet binary (BIP86 P2TR, local key management, no bitcoind wallets)
- [x] Full protocol simulation (`simulation`) with 3 merchants and 8 beneficiaries
- [x] On-chain registration fee verification (beneficiary + merchant)
- [x] Self-funded Taproot commitment transaction (aggregates beneficiary payment UTXOs)
- [x] Dynamic fee configuration via registry `GetFees` RPC
- [x] Registry wallet (secp256k1 keypair persisted in SQLite, P2TR address for fee collection)
- [x] Persistent storage for registry state (SQLite: merchants, sets, commitments, wallet key)

### Phase 3-4 improvements (Payment Identity Registration)
- [ ] Per-merchant nullifier duplicate rejection in the merchant service (currently verified in core but not enforced at the gRPC layer)
- [ ] Persistent storage for merchant registered identities (survives restarts)
- [ ] Privacy-preserving name binding — replace plaintext `friendly_name` with a commitment or blinded identifier to prevent cross-merchant linkage via name matching
- [ ] Batch payment identity registration (register with multiple merchants in a single flow)

### Phase 5 improvements (Payment Request)
- [ ] Payment amount limits and rate limiting per pseudonym
- [ ] Payment receipt / proof-of-payment from merchant back to beneficiary
- [ ] Multi-payment support — multiple sequential payments under the same pseudonym without re-registration
- [ ] On-chain P2TR spending path — enable beneficiaries to claim Taproot commitment outputs using their credential

### Infrastructure
- [ ] TLS for gRPC connections
- [ ] Credential revocation / proof expiry
- [ ] Anonymity set size scaling beyond current capacity
