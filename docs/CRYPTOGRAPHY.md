# Cryptographic Primitives

This document describes the core cryptographic building blocks used in the
Veiled protocol. For the full protocol flow, see [PROTOCOL.md](PROTOCOL.md).

---

## Multi-value Pedersen commitment

```
Φ = k·g + s_1·h_1 + ... + s_L·h_L + name_scalar·h_name
```

Where `g, h_1..h_L, h_name` are L+2 independent generators from the CRS.
Packs L nullifier values and a name hash into a single 33-byte curve point,
hidden by the blinding key k.

Properties:
- **Hiding**: given only Φ, an adversary cannot determine any s_l without k
- **Binding**: computationally infeasible to find different values producing
  the same Φ — one nullifier per merchant, enforced at the math level
- **Homomorphic**: required for Bootle/Groth membership proof

---

## CRS generator derivation

All generators are derived via hash-to-curve with distinct public strings:

```
g      = HashToCurve("CRS-ASC-generator-0",     DST="CRS-ASC-v1")  # base generator
h_1    = HashToCurve("CRS-ASC-generator-1",     DST="CRS-ASC-v1")  # merchant 1
h_2    = HashToCurve("CRS-ASC-generator-2",     DST="CRS-ASC-v1")  # merchant 2
...
h_name = HashToCurve("CRS-ASC-generator-name",  DST="CRS-ASC-v1")  # name generator
```

This is a NUMS (Nothing Up My Sleeve) construction — nobody knows the
discrete log of any generator relative to any other. If someone could
compute γ such that `h_l = γ·g`, they could break the binding property
of the commitment scheme.

---

## HKDF per-merchant nullifier derivation

```
s_l = HKDF-SHA256(IKM = sk, salt = merchant_name, info = "CRS-ASC-nullifier")
```

Different merchant names produce computationally independent scalars from the
same master secret. Two colluding merchants cannot determine from the nullifier
points alone that they came from the same user.

HKDF's unlinkability property guarantees that `HKDF(sk, v_1)` and
`HKDF(sk, v_2)` are computationally indistinguishable from independent
random values, even knowing both `v_1` and `v_2`.

---

## Public nullifier

```
nul_l = s_l · g     (scalar × generator = curve point)
```

Serves as both a Sybil-resistance token (same user always produces the same
`nul_l` for a given merchant) and a public authentication key. The nullifier
does double duty:

1. **Sybil resistance** — unique per master identity per merchant
2. **Authentication key** — the user can later prove knowledge of `s_l`

---

## One-out-of-many proof (Bootle/Groth 2015)

Proves knowledge of an index `j` and opening values such that `set[j]` is a
valid commitment, without revealing `j` or any secret values.

### Commitment shifting

The proof operates on **shifted commitments** that reduce the problem to a
commitment-to-zero membership test:

```
For each i = 1..N:
  D[i] = Φ_i - s_l·h_l
```

At the prover's index j, the merchant's term cancels:
```
D[j] = k·g + s_{j,2}·h_2 + ... + 0·h_l + ... + name·h_name
```

For all other indices i ≠ j, `s_{i,l} ≠ s_l` with overwhelming probability
(since nullifiers are independently derived from different secrets), so D[i]
has a non-zero l-th component.

### Proof structure

With N commitments in the anonymity set, the index j requires n = log₂(N)
bits. The proof proceeds in two rounds (made non-interactive via Fiat-Shamir):

**Round 1** — For each bit position k = 0..n-1:
- Sample fresh randomness: `r_k, a_k, s_k, t_k ←$ Z_q`
- Compute three commitments:
  ```
  A_k = j_k·g + r_k·h           # commitment to bit k of index j
  B_k = a_k·g + s_k·h           # commitment to random mask
  C_k = a_k·(1 - 2j_k)·g + t_k·h   # auxiliary for bitness argument
  ```
- Compute polynomial commitments `E_m` for m = 0..n-1:
  ```
  Q_m = Σ_{i=0}^{N-1} p_i^{(m)} · D_i
  E_m = Q_m + ρ_m · h
  ```

Where `p_i(x) = Π_{k=0}^{n-1} f_{k, i_k}(x)` encodes the index as a
polynomial that evaluates to 1 at i=j and 0 elsewhere.

**Challenge** (Fiat-Shamir):
```
x = Hash(crs, Λ, set_id, l, nul_l, ϕ, {A_k, B_k, C_k}, {E_m})
```

Including the pseudonym ϕ in the hash binds the proof to a specific
pseudonym — the proof cannot be reused for a different identity.

**Round 2** — For each bit k:
```
f_k   = j_k · x + a_k            # linear combination of bit and mask
z_A_k = r_k · x + s_k            # blinding response
z_C_k = r_k · (x - f_k) - t_k   # bitness response
```

Plus the polynomial evaluation: `z_E = Σ ρ_m · x^m + r_j · x^n`

### Verification

The verifier checks three types of equations:

1. **Bit consistency** (per bit k):
   `f_k·g + z_A_k·h == A_k·x + B_k`

2. **Bitness** (per bit k — proves j_k ∈ {0,1}):
   Checks that `f_k·(x - f_k) = 0` when the mask is removed, using the
   Schwartz-Zippel lemma — the equation is a polynomial identity that
   only holds for all x if the bit condition is satisfied.

3. **Membership** (single multi-scalar multiplication):
   `Σ E_m·x^m + z_E·h == Σ p_i(x)·D_i`

   The right side requires N evaluations of p_i(x) using the f_k values,
   then an N-point multi-scalar multiplication. This is the computational
   bottleneck: O(N) group operations.

### Nullifier correctness

A separate Schnorr proof (π_value) proves that `nul_l = s_l · g` is correctly
derived from the committed scalar at position l. This is a standard Schnorr
verification — one group equation.

### Proof size

On secp256k1 with N=8 (n=3):

| Component | Count | Size |
|-----------|-------|------|
| Group elements (A, B, C) | 3n = 9 | 297 bytes |
| Group elements (E) | n = 3 | 99 bytes |
| Scalars (f, z_A, z_C) | 3n = 9 | 288 bytes |
| Scalar (z_E) | 1 | 32 bytes |
| Schnorr proof (π_value) | 1 | ~65 bytes |
| **Total** | | **~781 bytes** |

At N=1024 (n=10): ~2.4 KB for the membership proof plus ~65 bytes for
nullifier correctness. The paper reports ~3.6 KB at N=1024 including
additional components in the DBPoE variant.

---

## Schnorr authentication (Phase 5)

Lightweight non-interactive proof of child credential knowledge, used for
payment requests after the initial registration:

```
t ←$ Z_q
R = t · g                                           # nonce commitment
e = SHA256("CRS-ASC-schnorr-child-auth" || g || ϕ || R)  # challenge
s = t + e · csk_l                                    # response

Proof = (R, s)
Verification: s·g == R + e·ϕ
```

Size: 33 bytes (R) + 32 bytes (s) = 65 bytes. Constant-time, independent
of anonymity set size.

---

## P2TR address derivation

Pseudonyms are secp256k1 public keys (`ϕ = csk · g`), which map directly to
Pay-to-Taproot (BIP341) Bitcoin addresses. The x-only public key is used as
the internal key with no script path, producing a standard P2TR output.

---

## Terminology

| Term | Meaning |
|------|---------|
| **CRS** | Common Reference String — public parameters `(g, h_1..h_L, h_name, v_1..v_L)` |
| **Beneficiary** | End user who receives payments — creates credential, registers identity |
| **Merchant** | Payment provider — verifies proofs, sends payments |
| **Master credential** | Tuple `(Φ, sk, r, k)` — beneficiary stores locally |
| **Master identity (Φ)** | `k·g + Σ s_l·h_l + name_scalar·h_name` — 33-byte Pedersen commitment |
| **Nullifier scalar (s_l)** | `HKDF(sk, merchant_name)` — 32-byte per-merchant scalar |
| **Public nullifier (nul_l)** | `s_l · g` — 33-byte curve point, Sybil resistance token |
| **Pseudonym (ϕ_l)** | `csk_l · g` — per-merchant public identity, unlinkable across merchants |
| **Child secret key (csk_l)** | `HKDF(r, merchant_name)` — per-merchant auth key |
| **Anonymity set (Λ)** | Fixed-size batch of commitments; sealed after finalization |
| **VTxO tree** | Binary tree of pre-signed Bitcoin transactions; root is on-chain, leaves are P2TR outputs |
| **P2TR address** | Pay-to-Taproot Bitcoin address derived from pseudonym |
| **Payment identity registration** | (ϕ, nul_l, proof, friendly_name) — submission to merchant |
| **NUMS** | Nothing Up My Sleeve — generators with no known discrete log relationships |
