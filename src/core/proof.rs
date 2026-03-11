//! Bootle/Groth one-out-of-many membership proof (Groth 2015).
//!
//! Proves in zero knowledge that a prover knows an index `l` and opening
//! `(v, r)` such that `C[l] = r·G + v·H` — i.e. the prover knows which
//! commitment in the anonymity set is theirs, without revealing `l`.
//!
//! Parameters: M = 10, N = 2^M = 1024.

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

use crate::core::commitment::h_generator;
use crate::core::types::{BlindingKey, Commitment, Nullifier};

// ── constants ─────────────────────────────────────────────────────────────────

const M: usize = 10; // bits
const N: usize = 1 << M; // 1024

// ── per-bit generators H_0 .. H_{M-1} ────────────────────────────────────────

fn bit_generators() -> [ProjectivePoint; M] {
    std::array::from_fn(|k| {
        let tag = format!("veiled-proof-H-{k}");
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[tag.as_bytes()],
            &[b"veiled-proof-v1"],
        )
        .expect("hash_to_curve never fails for secp256k1")
    })
}

// ── scalar helpers ────────────────────────────────────────────────────────────

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

// ── Fiat-Shamir challenge ─────────────────────────────────────────────────────

fn fiat_shamir_challenge(
    d_set: &[ProjectivePoint],
    a: &ProjectivePoint,
    b: &ProjectivePoint,
    c: &ProjectivePoint,
    d: &ProjectivePoint,
    g_vec: &[ProjectivePoint; M],
) -> Scalar {
    let mut hasher = Sha256::new();
    for di in d_set {
        hasher.update(point_to_bytes(di));
    }
    hasher.update(point_to_bytes(a));
    hasher.update(point_to_bytes(b));
    hasher.update(point_to_bytes(c));
    hasher.update(point_to_bytes(d));
    for gi in g_vec {
        hasher.update(point_to_bytes(gi));
    }
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
}

// ── proof structure ───────────────────────────────────────────────────────────

/// One-out-of-many membership proof (878 bytes).
///
/// Proves knowledge of index `l` and opening `(v, r)` such that `set[l] = r·G + v·H`.
pub struct MembershipProof {
    /// Commitment to random masks {a_k}.
    pub a: [u8; 33],
    /// Commitment to bits {l_k}.
    pub b: [u8; 33],
    /// Commitment to {a_k * (1 - 2*l_k)}.
    pub c: [u8; 33],
    /// Commitment to {-a_k^2}.
    pub d: [u8; 33],
    /// Polynomial decomposition points G_0..G_{M-1}.
    pub g: [[u8; 33]; M],
    /// Scalar evaluations f_k = l_k * x + a_k.
    pub f: [[u8; 32]; M],
    pub z_a: [u8; 32],
    pub z_c: [u8; 32],
    pub z:   [u8; 32],
}

// ── prove ─────────────────────────────────────────────────────────────────────

/// Generate a one-out-of-many membership proof.
///
/// - `set`: the full anonymity set of size N = 1024.
/// - `index`: the prover's position in `set` (0-based).
/// - `nullifier`: the nullifier value `v` (satisfies `set[index] = r·G + v·H`).
/// - `blinding`: the blinding factor `r`.
///
/// Returns `Err` if `set.len() != N` or `index >= N`.
pub fn prove_membership(
    set: &[Commitment],
    index: usize,
    nullifier: &Nullifier,
    blinding: &BlindingKey,
) -> Result<MembershipProof, &'static str> {
    if set.len() != N {
        return Err("set must have exactly 1024 commitments");
    }
    if index >= N {
        return Err("index out of range");
    }

    let g_gen = ProjectivePoint::GENERATOR;
    let h_gen = h_generator();
    let hk = bit_generators();

    // v = nullifier scalar, r_l = blinding scalar
    let v = bytes_to_scalar(&nullifier.0);
    let r_l = bytes_to_scalar(&blinding.0);

    // D[i] = C[i] - v·H  (shifted commitments; D[index] = r_l·G)
    let d_set: Vec<ProjectivePoint> = set
        .iter()
        .map(|c| {
            let pt = point_from_bytes(&c.0).expect("valid commitment");
            pt - h_gen * v
        })
        .collect();

    // Binary decomposition of index
    let bits: [Scalar; M] = std::array::from_fn(|k| {
        if (index >> k) & 1 == 1 { Scalar::ONE } else { Scalar::ZERO }
    });

    // Random scalars
    let a: [Scalar; M] = std::array::from_fn(|_| random_scalar());
    let r_a = random_scalar();
    let r_b = random_scalar();
    let r_c = random_scalar();
    let r_d = random_scalar();
    let rho: [Scalar; M] = std::array::from_fn(|_| random_scalar());

    // ── bit commitments ───────────────────────────────────────────────────────

    // A = r_a·G + Σ_k a[k]·H_k
    let mut cap_a = g_gen * r_a;
    for k in 0..M {
        cap_a += hk[k] * a[k];
    }

    // B = r_b·G + Σ_k bits[k]·H_k
    let mut cap_b = g_gen * r_b;
    for k in 0..M {
        cap_b += hk[k] * bits[k];
    }

    // C_com = r_c·G + Σ_k (a[k]*(1-2*bits[k]))·H_k
    let two = Scalar::from(2u64);
    let mut cap_c = g_gen * r_c;
    for k in 0..M {
        let coeff = a[k] * (Scalar::ONE - two * bits[k]);
        cap_c += hk[k] * coeff;
    }

    // D_com = r_d·G + Σ_k (-a[k]^2)·H_k
    let mut cap_d = g_gen * r_d;
    for k in 0..M {
        cap_d += hk[k] * (-a[k] * a[k]);
    }

    // ── polynomial decomposition points ──────────────────────────────────────
    //
    // For each j in 0..N, define the polynomial p_j(x) = Π_{k=0}^{M-1} f_{k,j_k}(x)
    //   where j_k = (j >> k) & 1
    //         f_{k,1}(x) = bits[k]*x + a[k]   (degree-1 in x)
    //         f_{k,0}(x) = (1-bits[k])*x - a[k]
    //
    // p_j is degree M in x. We need the coefficient of x^m for m=0..M-1 (not x^M).
    // G[m] = rho[m]·G + Σ_j coeff(p_j, x^m) · D[j]
    //
    // Efficient computation: for each j, expand p_j as a length-(M+1) coefficient
    // array, then for each m accumulate coeff[m]·D[j] into G[m].

    let mut cap_g = [ProjectivePoint::IDENTITY; M];
    for k in 0..M {
        cap_g[k] = g_gen * rho[k];
    }

    for j in 0..N {
        // Expand p_j coefficient vector (indices 0..=M, we only use 0..M)
        let mut poly = [Scalar::ZERO; M + 1];
        poly[0] = Scalar::ONE;
        let mut deg = 0usize;

        for k in 0..M {
            let jk = ((j >> k) & 1) as u64;
            // f_{k, jk}(x) = jk*bits[k]*x + a[k]  if jk==1  → bits[k]*x + a[k]
            //              = (1-bits[k])*x - a[k]  if jk==0
            let (coeff_x, coeff_0) = if jk == 1 {
                (bits[k], a[k])
            } else {
                (Scalar::ONE - bits[k], -a[k])
            };

            // poly = poly * (coeff_x * x + coeff_0)
            for i in (0..=deg).rev() {
                poly[i + 1] += poly[i] * coeff_x;
                poly[i] = poly[i] * coeff_0;
            }
            deg += 1;
        }

        // Accumulate coefficients x^0..x^{M-1} (skip x^M) into G[]
        let dj = d_set[j];
        for m in 0..M {
            cap_g[m] += dj * poly[m];
        }
    }

    // ── Fiat-Shamir challenge ─────────────────────────────────────────────────
    let x = fiat_shamir_challenge(&d_set, &cap_a, &cap_b, &cap_c, &cap_d, &cap_g);

    // ── responses ─────────────────────────────────────────────────────────────

    // f[k] = bits[k]*x + a[k]
    let f: [Scalar; M] = std::array::from_fn(|k| bits[k] * x + a[k]);

    // z_a = r_a + r_b*x
    let z_a = r_a + r_b * x;

    // z_c = r_c*x + r_d  (note: coefficient order matches f_k*(x-f_k) = x*C_coeff + D_coeff)
    let z_c = r_c * x + r_d;

    // z = r_l * x^M - Σ_k rho[k] * x^k
    let mut x_pow = Scalar::ONE; // x^0
    let mut sum_rho = Scalar::ZERO;
    for k in 0..M {
        sum_rho += rho[k] * x_pow;
        x_pow = x_pow * x;
    }
    // x_pow is now x^M
    let z = r_l * x_pow - sum_rho;

    Ok(MembershipProof {
        a:   point_to_bytes(&cap_a),
        b:   point_to_bytes(&cap_b),
        c:   point_to_bytes(&cap_c),
        d:   point_to_bytes(&cap_d),
        g:   std::array::from_fn(|k| point_to_bytes(&cap_g[k])),
        f:   std::array::from_fn(|k| scalar_to_bytes(&f[k])),
        z_a: scalar_to_bytes(&z_a),
        z_c: scalar_to_bytes(&z_c),
        z:   scalar_to_bytes(&z),
    })
}

// ── verify ────────────────────────────────────────────────────────────────────

/// Verify a one-out-of-many membership proof.
///
/// Returns `true` iff the proof is valid for the given `set` and `nullifier`.
pub fn verify_membership(
    set: &[Commitment],
    nullifier: &Nullifier,
    proof: &MembershipProof,
) -> bool {
    if set.len() != N {
        return false;
    }

    let g_gen = ProjectivePoint::GENERATOR;
    let h_gen = h_generator();
    let hk = bit_generators();

    let v = bytes_to_scalar(&nullifier.0);

    // D[i] = C[i] - v·H
    let d_set: Vec<ProjectivePoint> = set
        .iter()
        .map(|c| {
            let pt = match point_from_bytes(&c.0) {
                Some(p) => p,
                None => return ProjectivePoint::IDENTITY,
            };
            pt - h_gen * v
        })
        .collect();

    // Decode proof points
    let cap_a = match point_from_bytes(&proof.a) { Some(p) => p, None => return false };
    let cap_b = match point_from_bytes(&proof.b) { Some(p) => p, None => return false };
    let cap_c = match point_from_bytes(&proof.c) { Some(p) => p, None => return false };
    let cap_d = match point_from_bytes(&proof.d) { Some(p) => p, None => return false };
    let cap_g: [ProjectivePoint; M] = {
        let mut arr = [ProjectivePoint::IDENTITY; M];
        for k in 0..M {
            arr[k] = match point_from_bytes(&proof.g[k]) { Some(p) => p, None => return false };
        }
        arr
    };

    // Decode scalars
    let f: [Scalar; M] = std::array::from_fn(|k| scalar_from_bytes(&proof.f[k]));
    let z_a = scalar_from_bytes(&proof.z_a);
    let z_c = scalar_from_bytes(&proof.z_c);
    let z   = scalar_from_bytes(&proof.z);

    // Recompute challenge
    let x = fiat_shamir_challenge(&d_set, &cap_a, &cap_b, &cap_c, &cap_d, &cap_g);

    // ── Check 1: z_a·G == A + x·B - Σ_k f[k]·H_k ───────────────────────────
    let lhs1 = g_gen * z_a;
    let mut rhs1 = cap_a + cap_b * x;
    for k in 0..M {
        rhs1 -= hk[k] * f[k];
    }
    if lhs1.to_affine() != rhs1.to_affine() {
        return false;
    }

    // ── Check 2: z_c·G == x·C + D - Σ_k (f[k]*(x-f[k]))·H_k ───────────────
    // f[k]*(x-f[k]) = a[k]*(1-2*b[k])*x - a[k]^2, matching x*C_coeff + D_coeff
    let lhs2 = g_gen * z_c;
    let mut rhs2 = cap_c * x + cap_d;
    for k in 0..M {
        rhs2 -= hk[k] * (f[k] * (x - f[k]));
    }
    if lhs2.to_affine() != rhs2.to_affine() {
        return false;
    }

    // ── Check 3: Σ_j p_j(x)·D_j == z·G + Σ_{k=0}^{M-1} x^k·G[k] ──────────
    //
    // p_j(x) = Π_{k=0}^{M-1} ( j_k==1 ? f[k] : x - f[k] )
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

    let mut rhs3 = g_gen * z;
    let mut x_pow = Scalar::ONE;
    for k in 0..M {
        rhs3 += cap_g[k] * x_pow;
        x_pow = x_pow * x;
    }

    lhs3.to_affine() == rhs3.to_affine()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::commitment::commit;
    use crate::core::nullifier::compute_nullifier;
    use crate::core::types::{Name, PublicKey};

    fn make_set(size: usize, index: usize, nullifier: &Nullifier, blinding: &BlindingKey) -> Vec<Commitment> {
        let mut set = Vec::with_capacity(size);
        for i in 0..size {
            if i == index {
                set.push(commit(nullifier, blinding));
            } else {
                // dummy commitment: random-looking but deterministic
                let dummy_n = Nullifier([i as u8; 32]);
                let dummy_b = BlindingKey([(i + 1) as u8; 32]);
                set.push(commit(&dummy_n, &dummy_b));
            }
        }
        set
    }

    #[test]
    fn prove_and_verify_full_set() {
        let pub_key = PublicKey([0x42u8; 32]);
        let nullifier = compute_nullifier(&pub_key, &Name::new("alice"));
        let blinding = BlindingKey([0x07u8; 32]);

        let index = 42;
        let set = make_set(N, index, &nullifier, &blinding);

        let proof = prove_membership(&set, index, &nullifier, &blinding)
            .expect("prove succeeded");

        assert!(verify_membership(&set, &nullifier, &proof), "proof should verify");
    }

    #[test]
    fn wrong_nullifier_fails_verification() {
        let pub_key = PublicKey([0x42u8; 32]);
        let nullifier = compute_nullifier(&pub_key, &Name::new("alice"));
        let blinding = BlindingKey([0x07u8; 32]);
        let wrong_nullifier = compute_nullifier(&pub_key, &Name::new("bob"));

        let set = make_set(N, 5, &nullifier, &blinding);
        let proof = prove_membership(&set, 5, &nullifier, &blinding)
            .expect("prove succeeded");

        assert!(!verify_membership(&set, &wrong_nullifier, &proof));
    }

    #[test]
    fn tampered_proof_fails_verification() {
        let pub_key = PublicKey([0x11u8; 32]);
        let nullifier = compute_nullifier(&pub_key, &Name::new("charlie"));
        let blinding = BlindingKey([0x22u8; 32]);

        let set = make_set(N, 100, &nullifier, &blinding);
        let mut proof = prove_membership(&set, 100, &nullifier, &blinding)
            .expect("prove succeeded");

        // Flip a byte in z
        proof.z[0] ^= 0xFF;

        assert!(!verify_membership(&set, &nullifier, &proof));
    }

    #[test]
    fn wrong_set_size_returns_err() {
        let nullifier = Nullifier([0u8; 32]);
        let blinding = BlindingKey([1u8; 32]);
        let small_set: Vec<Commitment> = (0..8)
            .map(|i| commit(&Nullifier([i as u8; 32]), &BlindingKey([(i+1) as u8; 32])))
            .collect();
        assert!(prove_membership(&small_set, 0, &nullifier, &blinding).is_err());
    }
}
