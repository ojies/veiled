# Cryptographic Primitives

This document describes the core cryptographic building blocks used in the
Veiled protocol.

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

## HKDF per-merchant nullifier derivation

```
s_l = HKDF-SHA256(IKM = sk, salt = merchant_name, info = "CRS-ASC-nullifier")
```

Different merchant names produce computationally independent scalars from the
same master secret. Two colluding merchants cannot determine from the nullifier
points alone that they came from the same user.

---

## Public nullifier

```
nul_l = s_l · g     (scalar × generator = curve point)
```

Serves as both a Sybil-resistance token (same user always produces the same
`nul_l` for a given merchant) and a public authentication key.

---

## One-out-of-many proof (Bootle/Groth 2015)

Proves knowledge of an index `j` and opening values such that `set[j]` is a
valid commitment, without revealing `j` or any secret values. Adapted for
multi-value commitments with L+1 active generators and shifted commitments
`D[i] = Φ_i - s_l·h_l` to bind the proof to a specific merchant's nullifier.

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
| **Payment identity registration** | (ϕ, nul_l, proof, friendly_name) — Phase 3 submission to merchant |
