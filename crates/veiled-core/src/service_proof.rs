//! Adapted Bootle/Groth one-out-of-many proof for multi-value commitments (Phase 3).
//!
//! Proves membership in the anonymity set for a specific service provider,
//! using shifted commitments `D[i] = Φ_i - s_l · h_l` that reduce the
//! multi-value commitment to a commitment-to-zero at the prover's index.
//!
//! The witness is `(j, k, {s_m}_{m≠l})` — the blinding key plus L-1
//! nullifier scalars for all services except the target service `l`.
//!
//! Verification equations:
//! ```text
//! Check 1: z_a·g == A + x·B - Σ_k f[k]·H_k
//! Check 2: z_c·g == x·C + D - Σ_k (f[k]*(x-f[k]))·H_k
//! Check 3: Σ_j p_j(x)·D_j == z_g·g + Σ_{m≠l} z_m·h_m + Σ_k x^k·E[k]
//! ```
//!
//! Additionally includes π_value: a Schnorr proof that `nul_l = s_l · g`.
//!
//! Proof size: `911 + L×32` bytes.

use k256::{
    AffinePoint, ProjectivePoint,
    elliptic_curve::{
        group::GroupEncoding,
        hash2curve::{ExpandMsgXmd, GroupDigest},
        ops::Reduce,
    },
    Scalar, Secp256k1, U256,
};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::crs::Crs;
use crate::types::{Commitment, Nullifier};

// ── constants ─────────────────────────────────────────────────────────────────

const M: usize = 10; // bits
const N: usize = 1 << M; // 1024

/// CRS domain separation tag — shared with crs.rs.
const CRS_DST: &[u8] = b"CRS-ASC-v1";

// ── per-bit generators H_0 .. H_{M-1} ────────────────────────────────────────
//
// Independent NUMS generators for the compact aggregate bit commitment scheme.
// Derived via HashToCurve with CRS-consistent domain separation.

fn bit_generators() -> [ProjectivePoint; M] {
    std::array::from_fn(|k| {
        let tag = format!("CRS-ASC-bit-generator-{k}");
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[tag.as_bytes()],
            &[CRS_DST],
        )
        .expect("hash_to_curve never fails for secp256k1")
    })
}

// ── scalar / point helpers ───────────────────────────────────────────────────

fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(b))
}

fn random_scalar() -> Scalar {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    bytes_to_scalar(&buf)
}

fn scalar_to_bytes(s: &Scalar) -> [u8; 32] {
    s.to_bytes().into()
}

fn point_to_bytes(p: &ProjectivePoint) -> [u8; 33] {
    p.to_affine().to_bytes().into()
}

fn point_from_bytes(b: &[u8; 33]) -> Option<ProjectivePoint> {
    AffinePoint::from_bytes(b.into()).map(ProjectivePoint::from).into()
}

fn scalar_from_bytes(b: &[u8; 32]) -> Scalar {
    bytes_to_scalar(b)
}

// ── Fiat-Shamir challenge ───────────────────────────────────────────────────
//
// x = Hash(crs, Λ, d̂, l, nul_l, ϕ, {commitments}, {polynomial points})
// Spec section 3.5: "The inclusion of ϕ (the pseudonym) in the hash is
// critical — it binds the proof to a specific pseudonym."

fn fiat_shamir_challenge(
    crs_g: &ProjectivePoint,
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
    service_index: usize,
    set_id: u64,
    d_set: &[ProjectivePoint],
    a: &ProjectivePoint,
    b: &ProjectivePoint,
    c: &ProjectivePoint,
    d: &ProjectivePoint,
    e_poly: &[ProjectivePoint; M],
) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update(b"CRS-ASC-service-registration-v1");
    hasher.update(point_to_bytes(crs_g)); // bind to CRS
    hasher.update(pseudonym);             // ϕ
    hasher.update(public_nullifier);      // nul_l
    hasher.update(&(service_index as u64).to_be_bytes()); // l
    hasher.update(&set_id.to_be_bytes()); // d̂
    for di in d_set {
        hasher.update(point_to_bytes(di));
    }
    hasher.update(point_to_bytes(a));
    hasher.update(point_to_bytes(b));
    hasher.update(point_to_bytes(c));
    hasher.update(point_to_bytes(d));
    for ei in e_poly {
        hasher.update(point_to_bytes(ei));
    }
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
}

// ── Schnorr challenge for π_value ───────────────────────────────────────────
//
// Spec section 3.7: "π_value = Schnorr proof that nul_l = s_l·g"
// e = Hash("CRS-ASC-schnorr-nullifier" || g || nul_l || R)

fn schnorr_challenge(
    g: &ProjectivePoint,
    public_nullifier: &[u8; 33],
    r_point: &ProjectivePoint,
) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update(b"CRS-ASC-schnorr-nullifier");
    hasher.update(point_to_bytes(g));
    hasher.update(public_nullifier);
    hasher.update(point_to_bytes(r_point));
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
}

// ── proof structure ─────────────────────────────────────────────────────────

/// Service registration proof adapted for multi-value commitments.
///
/// Proves that the prover knows the opening of one of the shifted
/// commitments `D[j] = k·g + Σ_{m≠l} s_m·h_m` without revealing `j`.
///
/// Components:
/// - π_membership: compact Bootle/Groth one-out-of-many proof (A, B, C, D, E, f, z)
/// - π_value: Schnorr proof that `nul_l = s_l · g` (section 3.7)
///
/// Proof size: `911 + L×32` bytes where L = number of service providers.
#[derive(Clone)]
pub struct ServiceRegistrationProof {
    // ── π_membership: Bootle/Groth one-out-of-many ─────────────────────────

    /// Aggregate bit commitment A = r_a·g + Σ_k a[k]·H_k.
    pub a: [u8; 33],
    /// Aggregate bit commitment B = r_b·g + Σ_k j_k·H_k.
    pub b: [u8; 33],
    /// Aggregate cross-term C = r_c·g + Σ_k (a[k]*(1-2j_k))·H_k.
    pub c: [u8; 33],
    /// Aggregate square commitment D = r_d·g + Σ_k (-a[k]^2)·H_k.
    pub d: [u8; 33],
    /// Polynomial decomposition points E_0..E_{M-1} (spec: {E_m}_{m=0}^{n-1}).
    pub e_poly: [[u8; 33]; M],
    /// Scalar evaluations f_k = j_k·x + a_k for k = 0..M-1 (spec: section 3.6).
    pub f: [[u8; 32]; M],
    /// Bit blinding response z_a = r_a + r_b·x.
    pub z_a: [u8; 32],
    /// Cross-term response z_c = r_c·x + r_d.
    pub z_c: [u8; 32],
    /// Multi-generator polynomial responses: [z_g, z_{m≠l}].
    /// z_g = k·x^M - Σ_k ρ_g[k]·x^k
    /// z_m = s_m·x^M - Σ_k ρ_m[k]·x^k (for each m ≠ l)
    /// Length = L (1 for g + L-1 for h_m where m≠l).
    pub z_responses: Vec<[u8; 32]>,

    // ── π_value: Schnorr proof that nul_l = s_l · g (section 3.7) ──────────

    /// Schnorr nonce commitment R = t·g.
    pub schnorr_r: [u8; 33],
    /// Schnorr response s = t + e·s_l.
    pub schnorr_s: [u8; 32],
}

// ── prove ───────────────────────────────────────────────────────────────────

/// Generate a service registration proof for a multi-value commitment.
///
/// - `crs`: the Common Reference String.
/// - `anonymity_set`: the full anonymity set of size N = 1024.
/// - `index`: the prover's position in the set (0-based).
/// - `service_index`: which service to register for (1-indexed).
/// - `blinding_key`: `k` — the Pedersen blinding key.
/// - `nullifier_scalars`: all L nullifier scalars `s_1..s_L`.
/// - `pseudonym`: `ϕ = csk_l · g` (33-byte compressed point).
/// - `public_nullifier`: `nul_l = s_l · g` (33-byte compressed point).
pub fn prove_service_registration(
    crs: &Crs,
    anonymity_set: &[Commitment],
    index: usize,
    service_index: usize,
    set_id: u64,
    blinding_key: &[u8; 32],
    nullifier_scalars: &[Nullifier],
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
) -> Result<ServiceRegistrationProof, &'static str> {
    if anonymity_set.len() != N {
        return Err("set must have exactly 1024 commitments");
    }
    if index >= N {
        return Err("index out of range");
    }
    let l_count = crs.num_providers();
    if service_index < 1 || service_index > l_count {
        return Err("service_index out of range (1-indexed)");
    }
    if nullifier_scalars.len() != l_count {
        return Err("nullifier_scalars count must equal number of providers");
    }

    let g = crs.g;
    let hk = bit_generators();

    // The nullifier scalar for the target service
    let s_l = bytes_to_scalar(&nullifier_scalars[service_index - 1].0);
    let h_l = *crs.h(service_index);

    // ── Shifted commitments: D[i] = Φ_i - s_l · h_l (section 3.3) ───────────
    let shift = h_l * s_l;
    let d_set: Vec<ProjectivePoint> = anonymity_set
        .iter()
        .map(|c| {
            let pt = point_from_bytes(&c.0).expect("valid commitment");
            pt - shift
        })
        .collect();

    // At D[index], the l-th component cancels:
    // D[j] = k·g + Σ_{m≠l} s_m·h_m
    let k_scalar = bytes_to_scalar(blinding_key);

    // ── Binary decomposition of index (section 3.4) ──────────────────────────
    let bits: [Scalar; M] = std::array::from_fn(|k| {
        if (index >> k) & 1 == 1 { Scalar::ONE } else { Scalar::ZERO }
    });

    // ── Random scalars (section 3.4: r_k, a_k, s_k, t_k per bit) ────────────
    let a: [Scalar; M] = std::array::from_fn(|_| random_scalar());
    let r_a = random_scalar();
    let r_b = random_scalar();
    let r_c = random_scalar();
    let r_d = random_scalar();

    // Multi-generator blinding: ρ[gen_idx][k] (section 3.4: polynomial blinding)
    // gen_idx 0 = g, gen_idx 1..L-1 = h_m for m ≠ l (in order)
    let num_active_gens = l_count; // g + (L-1) h_m's = L total
    let rho: Vec<[Scalar; M]> = (0..num_active_gens)
        .map(|_| std::array::from_fn(|_| random_scalar()))
        .collect();

    // ── Bit commitments (compact aggregate variant) ──────────────────────────
    // Uses CRS base generator g as the blinding generator (spec: A_k = j_k·g + r_k·h)

    // A = r_a·g + Σ_k a[k]·H_k
    let mut cap_a = g * r_a;
    for k in 0..M {
        cap_a += hk[k] * a[k];
    }

    // B = r_b·g + Σ_k bits[k]·H_k
    let mut cap_b = g * r_b;
    for k in 0..M {
        cap_b += hk[k] * bits[k];
    }

    // C = r_c·g + Σ_k (a[k]*(1-2*bits[k]))·H_k
    let two = Scalar::from(2u64);
    let mut cap_c = g * r_c;
    for k in 0..M {
        let coeff = a[k] * (Scalar::ONE - two * bits[k]);
        cap_c += hk[k] * coeff;
    }

    // D = r_d·g + Σ_k (-a[k]^2)·H_k
    let mut cap_d = g * r_d;
    for k in 0..M {
        cap_d += hk[k] * (-a[k] * a[k]);
    }

    // ── Build list of active generators (g, then h_m for m≠l) ────────────────
    let mut active_gens: Vec<ProjectivePoint> = Vec::with_capacity(num_active_gens);
    active_gens.push(g); // index 0 = CRS g
    for m in 1..=l_count {
        if m != service_index {
            active_gens.push(*crs.h(m));
        }
    }
    debug_assert_eq!(active_gens.len(), num_active_gens);

    // ── Polynomial decomposition points (section 3.4: E_m = Q_m + blinding) ──
    // E[k] = Σ_{gen} ρ[gen][k]·active_gens[gen] + Σ_j coeff(p_j, x^k)·D[j]

    let mut cap_e = [ProjectivePoint::IDENTITY; M];
    for k in 0..M {
        for (gen_idx, gen_point) in active_gens.iter().enumerate() {
            cap_e[k] += *gen_point * rho[gen_idx][k];
        }
    }

    for j in 0..N {
        let mut poly = [Scalar::ZERO; M + 1];
        poly[0] = Scalar::ONE;
        let mut deg = 0usize;

        for k in 0..M {
            let jk = ((j >> k) & 1) as u64;
            let (coeff_x, coeff_0) = if jk == 1 {
                (bits[k], a[k])
            } else {
                (Scalar::ONE - bits[k], -a[k])
            };

            for i in (0..=deg).rev() {
                poly[i + 1] += poly[i] * coeff_x;
                poly[i] = poly[i] * coeff_0;
            }
            deg += 1;
        }

        let dj = d_set[j];
        for m in 0..M {
            cap_e[m] += dj * poly[m];
        }
    }

    // ── Fiat-Shamir challenge (section 3.5) ──────────────────────────────────
    let x = fiat_shamir_challenge(
        &g,
        pseudonym,
        public_nullifier,
        service_index,
        set_id,
        &d_set,
        &cap_a, &cap_b, &cap_c, &cap_d,
        &cap_e,
    );

    // ── π_membership responses (section 3.6) ─────────────────────────────────

    // f[k] = j_k·x + a_k
    let f: [Scalar; M] = std::array::from_fn(|k| bits[k] * x + a[k]);

    // z_a = r_a + r_b·x
    let z_a = r_a + r_b * x;

    // z_c = r_c·x + r_d
    let z_c = r_c * x + r_d;

    // Multi-generator z responses (multi-generator extension of z_E):
    // z_g = k·x^M - Σ_k ρ_g[k]·x^k
    // z_m = s_m·x^M - Σ_k ρ_m[k]·x^k (for each m ≠ l)
    let mut x_pow = Scalar::ONE;
    let mut x_powers = [Scalar::ZERO; M + 1];
    for k in 0..=M {
        x_powers[k] = x_pow;
        x_pow = x_pow * x;
    }

    // Build witness scalars: [k, s_1, ..., s_{l-1}, s_{l+1}, ..., s_L]
    let mut witness_scalars: Vec<Scalar> = Vec::with_capacity(num_active_gens);
    witness_scalars.push(k_scalar); // for g
    for m in 1..=l_count {
        if m != service_index {
            witness_scalars.push(bytes_to_scalar(&nullifier_scalars[m - 1].0));
        }
    }

    let mut z_responses: Vec<[u8; 32]> = Vec::with_capacity(num_active_gens);
    for gen_idx in 0..num_active_gens {
        let mut sum_rho = Scalar::ZERO;
        for k in 0..M {
            sum_rho += rho[gen_idx][k] * x_powers[k];
        }
        let z = witness_scalars[gen_idx] * x_powers[M] - sum_rho;
        z_responses.push(scalar_to_bytes(&z));
    }

    // ── π_value: Schnorr proof that nul_l = s_l · g (section 3.7) ───────────
    let schnorr_nonce = random_scalar();
    let schnorr_r_point = g * schnorr_nonce;
    let schnorr_e = schnorr_challenge(&g, public_nullifier, &schnorr_r_point);
    let schnorr_s_scalar = schnorr_nonce + schnorr_e * s_l;

    Ok(ServiceRegistrationProof {
        a:   point_to_bytes(&cap_a),
        b:   point_to_bytes(&cap_b),
        c:   point_to_bytes(&cap_c),
        d:   point_to_bytes(&cap_d),
        e_poly: std::array::from_fn(|k| point_to_bytes(&cap_e[k])),
        f:   std::array::from_fn(|k| scalar_to_bytes(&f[k])),
        z_a: scalar_to_bytes(&z_a),
        z_c: scalar_to_bytes(&z_c),
        z_responses,
        schnorr_r: point_to_bytes(&schnorr_r_point),
        schnorr_s: scalar_to_bytes(&schnorr_s_scalar),
    })
}

// ── verify ──────────────────────────────────────────────────────────────────

/// Verify a service registration proof.
///
/// - `crs`: the Common Reference String.
/// - `anonymity_set`: the full anonymity set.
/// - `service_index`: which service (1-indexed).
/// - `nullifier_scalar`: the revealed `s_l`.
/// - `pseudonym`: `ϕ = csk_l · g` (33-byte compressed point).
/// - `public_nullifier`: `nul_l = s_l · g` (33-byte compressed point).
/// - `proof`: the proof to verify.
pub fn verify_service_registration(
    crs: &Crs,
    anonymity_set: &[Commitment],
    service_index: usize,
    set_id: u64,
    nullifier_scalar: &Nullifier,
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
    proof: &ServiceRegistrationProof,
) -> bool {
    if anonymity_set.len() != N {
        return false;
    }
    let l_count = crs.num_providers();
    if service_index < 1 || service_index > l_count {
        return false;
    }
    // z_responses should have L entries (g + L-1 h_m's)
    if proof.z_responses.len() != l_count {
        return false;
    }

    let g = crs.g;
    let hk = bit_generators();

    // ── Verify nul_l = s_l · g (trivially checkable since s_l is revealed) ───
    let s_l = bytes_to_scalar(&nullifier_scalar.0);
    let expected_nul = point_to_bytes(&(g * s_l));
    if expected_nul != *public_nullifier {
        return false;
    }

    // ── Verify π_value: Schnorr proof that nul_l = s_l · g (section 3.7) ────
    let schnorr_r = match point_from_bytes(&proof.schnorr_r) { Some(p) => p, None => return false };
    let schnorr_s = scalar_from_bytes(&proof.schnorr_s);
    let schnorr_e = schnorr_challenge(&g, public_nullifier, &schnorr_r);
    let nul_point = match point_from_bytes(public_nullifier) { Some(p) => p, None => return false };

    // Verify: s·g == R + e·nul_l
    let schnorr_lhs = g * schnorr_s;
    let schnorr_rhs = schnorr_r + nul_point * schnorr_e;
    if schnorr_lhs.to_affine() != schnorr_rhs.to_affine() {
        return false;
    }

    // ── Compute shifted set D[i] = Φ_i - s_l · h_l (section 3.3) ───────────
    let h_l = *crs.h(service_index);
    let shift = h_l * s_l;
    let d_set: Vec<ProjectivePoint> = anonymity_set
        .iter()
        .map(|c| {
            match point_from_bytes(&c.0) {
                Some(pt) => pt - shift,
                None => ProjectivePoint::IDENTITY,
            }
        })
        .collect();

    // ── Decode proof points ─────────────────────────────────────────────────
    let cap_a = match point_from_bytes(&proof.a) { Some(p) => p, None => return false };
    let cap_b = match point_from_bytes(&proof.b) { Some(p) => p, None => return false };
    let cap_c = match point_from_bytes(&proof.c) { Some(p) => p, None => return false };
    let cap_d = match point_from_bytes(&proof.d) { Some(p) => p, None => return false };
    let cap_e: [ProjectivePoint; M] = {
        let mut arr = [ProjectivePoint::IDENTITY; M];
        for k in 0..M {
            arr[k] = match point_from_bytes(&proof.e_poly[k]) { Some(p) => p, None => return false };
        }
        arr
    };

    let f: [Scalar; M] = std::array::from_fn(|k| scalar_from_bytes(&proof.f[k]));
    let z_a = scalar_from_bytes(&proof.z_a);
    let z_c = scalar_from_bytes(&proof.z_c);

    // ── Recompute Fiat-Shamir challenge (section 3.5) ───────────────────────
    let x = fiat_shamir_challenge(
        &g,
        pseudonym,
        public_nullifier,
        service_index,
        set_id,
        &d_set,
        &cap_a, &cap_b, &cap_c, &cap_d,
        &cap_e,
    );

    // ── Check 1: z_a·g == A + x·B - Σ_k f[k]·H_k (section 3.6) ───────────
    let lhs1 = g * z_a;
    let mut rhs1 = cap_a + cap_b * x;
    for k in 0..M {
        rhs1 -= hk[k] * f[k];
    }
    if lhs1.to_affine() != rhs1.to_affine() {
        return false;
    }

    // ── Check 2: z_c·g == x·C + D - Σ_k (f[k]*(x-f[k]))·H_k ──────────────
    // Bitness check: f_k·(x-f_k) = j_k(1-j_k)·x^2 + a_k(1-2j_k)·x - a_k^2
    // Only holds if j_k ∈ {0,1} (spec section 3.6).
    let lhs2 = g * z_c;
    let mut rhs2 = cap_c * x + cap_d;
    for k in 0..M {
        rhs2 -= hk[k] * (f[k] * (x - f[k]));
    }
    if lhs2.to_affine() != rhs2.to_affine() {
        return false;
    }

    // ── Check 3: Σ_j p_j(x)·D_j == z_g·g + Σ_{m≠l} z_m·h_m + Σ_k x^k·E[k]
    // Multi-generator polynomial evaluation check (section 3.4/3.6 adapted).
    let mut lhs3 = ProjectivePoint::IDENTITY;
    for j in 0..N {
        let mut pj = Scalar::ONE;
        for k in 0..M {
            let jk = ((j >> k) & 1) as u64;
            if jk == 1 {
                pj = pj * f[k];
            } else {
                pj = pj * (x - f[k]);
            }
        }
        lhs3 += d_set[j] * pj;
    }

    // RHS: z_g·g + Σ_{m≠l} z_m·h_m + Σ_k x^k·E[k]
    let mut rhs3 = ProjectivePoint::IDENTITY;

    // z_g · g
    let z_g = scalar_from_bytes(&proof.z_responses[0]);
    rhs3 += g * z_g;

    // z_m · h_m for m ≠ l
    let mut resp_idx = 1;
    for m in 1..=l_count {
        if m != service_index {
            let z_m = scalar_from_bytes(&proof.z_responses[resp_idx]);
            rhs3 += *crs.h(m) * z_m;
            resp_idx += 1;
        }
    }

    // Σ_k x^k · E[k]
    let mut x_pow = Scalar::ONE;
    for k in 0..M {
        rhs3 += cap_e[k] * x_pow;
        x_pow = x_pow * x;
    }

    lhs3.to_affine() == rhs3.to_affine()
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child_credential::derive_pseudonym;
    use crate::credential::MasterCredential;
    use crate::crs::ServiceProvider;
    use crate::nullifier_v2::derive_public_nullifier;
    use crate::types::{BlindingKey, ChildRandomness, MasterSecret, Name};

    fn make_provider(username: &str) -> ServiceProvider {
        ServiceProvider {
            username: Name::new(username),
            credential_generator: [0x02; 33],
            origin: format!("https://{username}"),
        }
    }

    fn make_crs(n: usize) -> Crs {
        let providers: Vec<ServiceProvider> = (0..n)
            .map(|i| make_provider(&format!("user-{i}")))
            .collect();
        Crs::setup(providers)
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        let sk = MasterSecret([seed; 32]);
        let r = ChildRandomness([seed.wrapping_add(1); 32]);
        let k = BlindingKey([seed.wrapping_add(2); 32]);
        MasterCredential::create(crs, sk, r, k)
    }

    fn make_full_set(crs: &Crs, target_seed: u8, target_pos: usize) -> (MasterCredential, Vec<Commitment>) {
        let target = make_credential(crs, target_seed);
        let mut set = Vec::with_capacity(N);
        for i in 0..N {
            if i == target_pos {
                set.push(target.phi);
            } else {
                // Unique dummy credentials — seed must not collide with target_seed
                let seed = if (i as u8) == target_seed {
                    (i as u8).wrapping_add(128)
                } else {
                    i as u8
                };
                set.push(make_credential(crs, seed).phi);
            }
        }
        (target, set)
    }

    const TEST_SET_ID: u64 = 0;

    #[test]
    fn prove_and_verify_service_registration() {
        let crs = make_crs(3);
        let target_pos = 42;
        let (cred, set) = make_full_set(&crs, 0xAA, target_pos);

        let service_index = 2; // 1-indexed
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].username, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, service_index);

        let proof = prove_service_registration(
            &crs,
            &set,
            target_pos,
            service_index,
            TEST_SET_ID,
            &cred.k.0,
            &all_nullifiers,
            &pseudonym,
            &pub_nul,
        )
        .expect("proof generation should succeed");

        assert!(
            verify_service_registration(
                &crs,
                &set,
                service_index,
                TEST_SET_ID,
                &all_nullifiers[service_index - 1],
                &pseudonym,
                &pub_nul,
                &proof,
            ),
            "valid proof should verify"
        );
    }

    #[test]
    fn wrong_nullifier_scalar_fails() {
        let crs = make_crs(3);
        let (cred, set) = make_full_set(&crs, 0xBB, 10);

        let service_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].username, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, service_index);

        let proof = prove_service_registration(
            &crs, &set, 10, service_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
        )
        .unwrap();

        // Use wrong nullifier scalar
        let wrong_nul = Nullifier([0xFF; 32]);
        let wrong_pub = derive_public_nullifier(
            &MasterSecret([0xFF; 32]),
            &crs.providers[0].username,
            &crs.g,
        );
        assert!(!verify_service_registration(
            &crs, &set, service_index, TEST_SET_ID, &wrong_nul,
            &pseudonym, &wrong_pub, &proof,
        ));
    }

    #[test]
    fn wrong_service_index_fails() {
        let crs = make_crs(3);
        let (cred, set) = make_full_set(&crs, 0xCC, 100);

        let service_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].username, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, service_index);

        let proof = prove_service_registration(
            &crs, &set, 100, service_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
        )
        .unwrap();

        // Verify at different service index — should fail
        let other_service = 2;
        let other_nul_scalar = &all_nullifiers[other_service - 1];
        let other_pub_nul = cred.public_nullifier(&crs, other_service);
        assert!(!verify_service_registration(
            &crs, &set, other_service, TEST_SET_ID, other_nul_scalar,
            &pseudonym, &other_pub_nul, &proof,
        ));
    }

    #[test]
    fn tampered_proof_fails() {
        let crs = make_crs(2);
        let (cred, set) = make_full_set(&crs, 0xDD, 500);

        let service_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].username, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, service_index);

        let mut proof = prove_service_registration(
            &crs, &set, 500, service_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
        )
        .unwrap();

        // Flip a byte in z_responses
        proof.z_responses[0][0] ^= 0xFF;

        assert!(!verify_service_registration(
            &crs, &set, service_index, TEST_SET_ID,
            &all_nullifiers[service_index - 1],
            &pseudonym, &pub_nul, &proof,
        ));
    }

    #[test]
    fn proof_size_matches_expected() {
        let l_count = 4;
        let crs = make_crs(l_count);
        let (cred, set) = make_full_set(&crs, 0xEE, 0);

        let service_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].username, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, service_index);

        let proof = prove_service_registration(
            &crs, &set, 0, service_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
        )
        .unwrap();

        // π_membership fixed: 4×33 (ABCD) + 10×33 (E_poly) + 10×32 (f) + 2×32 (z_a, z_c)
        //   = 132 + 330 + 320 + 64 = 846
        // π_membership variable: L×32 (z_responses)
        // π_value: 33 (schnorr_r) + 32 (schnorr_s) = 65
        // Total: 911 + L×32
        let expected = 911 + l_count * 32;
        let actual = 4 * 33 + M * 33 + M * 32 + 2 * 32
            + proof.z_responses.len() * 32
            + 33 + 32; // schnorr_r + schnorr_s
        assert_eq!(actual, expected);
        assert_eq!(proof.z_responses.len(), l_count);
    }
}
