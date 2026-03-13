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
//! Proof size: `PROOF_BASE_SIZE + (L+1)×32` bytes.


use crate::core::crs::Crs;
use crate::core::types::{ Commitment, Nullifier};
use crate::core::utils::{M, N, bit_generators, bytes_to_scalar, fiat_shamir_challenge, point_from_bytes, point_to_bytes, random_scalar, scalar_from_bytes, scalar_to_bytes, schnorr_challenge};
use k256::Scalar;
use k256::{
     ProjectivePoint,

};






/// The message sent from a prover to a verifier during Phase 3.
///
/// ```text
/// (ϕ, nul_l, π, d̂, friendly_name)
/// ```
///
/// The nullifier scalar `s_l` and name scalar `SHA256(friendly_name)` are
/// embedded inside π (self-contained proof).
#[derive(Debug, Clone)]
pub struct PaymentIdentityRegistration {
    /// Pseudonym `ϕ = csk_l · g` — the user's public identity at this service.
    pub pseudonym: [u8; 33],
    /// Public nullifier `nul_l = s_l · g` — Sybil resistance token.
    pub public_nullifier: [u8; 33],
    /// `d̂` — which anonymity set the user is in.
    pub set_id: u64,
    /// Which service this registration is for (1-indexed).
    pub service_index: usize,
    /// The prover's revealed friendly name — verifier checks
    /// `SHA256(friendly_name) == proof.name_scalar`.
    pub friendly_name: String,
    /// The adapted Bootle/Groth membership proof over shifted commitments.
    /// Contains the embedded nullifier scalar `s_l` and name scalar.
    pub proof: crate::core::payment_identity::PaymentIdentityRegistrationProof,
}


/// Verify a complete service registration message.
///
/// The verifier needs the CRS and the frozen anonymity set `Λ_{d̂}`.
pub fn verify_payment_identity_registration(
    crs: &Crs,
    anonymity_set: &[Commitment],
    reg: &PaymentIdentityRegistration,
) -> bool {
    crate::core::payment_identity::verify_payment_identity_registration_proof(
        crs,
        anonymity_set,
        reg.service_index,
        reg.set_id,
        &reg.pseudonym,
        &reg.public_nullifier,
        &reg.proof,
    )
}


/// Verify that a claimed friendly name matches the name_scalar in a proof.
///
/// Bob reveals his friendly_name alongside the proof. Alice checks:
/// ```text
/// SHA256(friendly_name) == proof.name_scalar
/// ```
///
/// This is secure because `name_scalar` is bound to the proof via
/// Fiat-Shamir — if the prover embeds a fake name_scalar, the challenge
/// changes and the proof fails.
pub fn verify_name_revelation(
    proof_name_scalar: &[u8; 32],
    claimed_name: &str,
) -> bool {
    use sha2::{Digest, Sha256};
    let computed: [u8; 32] = Sha256::digest(claimed_name.as_bytes()).into();
    computed == *proof_name_scalar
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
/// Proof size: `PROOF_BASE_SIZE + (L+1)×32` bytes where L = number of service providers.


#[derive(Debug, Clone)]
pub struct PaymentIdentityRegistrationProof {
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
    /// Length = L+1 (1 for g + L-1 for h_m where m≠l + 1 for h_name).
    pub z_responses: Vec<[u8; 32]>,

    // ── π_value: Schnorr proof that nul_l = s_l · g (section 3.7) ──────────

    /// Schnorr nonce commitment R = t·g.
    pub schnorr_r: [u8; 33],
    /// Schnorr response s = t + e·s_l.
    pub schnorr_s: [u8; 32],

    // ── Revealed scalar (embedded in π per spec) ────────────────────────────

    /// The revealed nullifier scalar `s_l` — embedded in π.
    /// Step 4.6: π_value proves `nul_l = s_l · g`.
    pub nullifier_scalar: [u8; 32],

    // ── Revealed name (bound via Fiat-Shamir) ────────────────────────────────

    /// The name scalar `SHA256(friendly_name)` — bound to the proof via
    /// Fiat-Shamir. If the prover lies about the name, the challenge changes
    /// and the proof fails.
    pub name_scalar: [u8; 32],
}

// ── prove ───────────────────────────────────────────────────────────────────

/// Generate a service registration proof for a multi-value commitment.
///
/// - `crs`: the Common Reference String.
/// - `anonymity_set`: the full anonymity set of size N.
/// - `index`: the prover's position in the set (0-based).
/// - `service_index`: which service to register for (1-indexed).
/// - `blinding_key`: `k` — the Pedersen blinding key.
/// - `nullifier_scalars`: all L nullifier scalars `s_1..s_L`.
/// - `pseudonym`: `ϕ = csk_l · g` (33-byte compressed point).
/// - `public_nullifier`: `nul_l = s_l · g` (33-byte compressed point).
pub fn prove_payment_identity_registration(
    crs: &Crs,
    anonymity_set: &[Commitment],
    index: usize,
    service_index: usize,
    set_id: u64,
    blinding_key: &[u8; 32],
    nullifier_scalars: &[Nullifier],
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
    name_scalar: &[u8; 32],
) -> Result<PaymentIdentityRegistrationProof, &'static str> {
    if anonymity_set.len() != N {
        return Err("set must be a power of 2 with exactly N commitments");
    }
    if index >= N {
        return Err("index out of range");
    }
    let l_count = crs.num_merchants();
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
    // gen_idx 0 = g, gen_idx 1..L-1 = h_m for m ≠ l, gen_idx L = h_name
    let num_active_gens = l_count + 1; // g + (L-1) h_m's + h_name = L+1 total
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

    // ── Build list of active generators (g, h_{m≠l}, h_name) ──────────────────
    let mut active_gens: Vec<ProjectivePoint> = Vec::with_capacity(num_active_gens);
    active_gens.push(g); // index 0 = CRS g
    for m in 1..=l_count {
        if m != service_index {
            active_gens.push(*crs.h(m));
        }
    }
    active_gens.push(crs.h_name); // last = h_name
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
        name_scalar,
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

    // Build witness scalars: [k, s_{m≠l}, name_scalar]
    let mut witness_scalars: Vec<Scalar> = Vec::with_capacity(num_active_gens);
    witness_scalars.push(k_scalar); // for g
    for m in 1..=l_count {
        if m != service_index {
            witness_scalars.push(bytes_to_scalar(&nullifier_scalars[m - 1].0));
        }
    }
    witness_scalars.push(bytes_to_scalar(name_scalar)); // for h_name

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

    Ok(PaymentIdentityRegistrationProof {
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
        nullifier_scalar: nullifier_scalars[service_index - 1].0,
        name_scalar: *name_scalar,
    })
}

// ── verify ──────────────────────────────────────────────────────────────────

/// Verify a service registration proof (steps 4.2–4.6).
///
/// The verifier (Bob, user `l`) receives `(ϕ, nul_l, π, d̂)` from the
/// prover (Alice). The proof π contains the nullifier scalar `s_l`
/// internally, so the verifier does not need it as a separate parameter.
///
/// Steps (spec section 4):
/// - 4.2: Recompute shifted commitments `D_i = Φ_i - s_l · h_l`
/// - 4.3: Recompute Fiat-Shamir challenge `x`
/// - 4.4: Verify 10 bitness equations (Check 1 + Check 2)
/// - 4.5: Verify polynomial identity — O(N) group ops (Schwartz-Zippel)
/// - 4.6: Verify nullifier correctness — `nul_l = s_l · g` (Schnorr π_value)
pub fn verify_payment_identity_registration_proof(
    crs: &Crs,
    anonymity_set: &[Commitment],
    service_index: usize,
    set_id: u64,
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
    proof: &PaymentIdentityRegistrationProof,
) -> bool {
    if anonymity_set.len() != N {
        return false;
    }
    let l_count = crs.num_merchants();
    if service_index < 1 || service_index > l_count {
        return false;
    }
    // z_responses should have L+1 entries (g + L-1 h_m's + h_name)
    if proof.z_responses.len() != l_count + 1 {
        return false;
    }

    let g = crs.g;
    let hk = bit_generators();

    // Extract s_l from the proof (π contains the revealed nullifier scalar).
    let s_l = bytes_to_scalar(&proof.nullifier_scalar);

    // ── 4.2 Recompute the Shifted Commitments ───────────────────────────────
    // D_i = Φ_i - s_l · h_l  for all i = 1..N
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

    // ── 4.3 Recompute the Fiat-Shamir Challenge ────────────────────────────
    // x = Hash(crs, Λ_{d̂}, d̂, l, nul_l, ϕ, name_scalar, {A,B,C,D}, {E_m})
    let x = fiat_shamir_challenge(
        &g,
        pseudonym,
        public_nullifier,
        service_index,
        set_id,
        &proof.name_scalar,
        &d_set,
        &cap_a, &cap_b, &cap_c, &cap_d,
        &cap_e,
    );

    // ── 4.4 Verify the Bit Commitments ──────────────────────────────────────

    // Check 1 — Consistency of f_k with A and B:
    //   z_a·g == A + x·B - Σ_k f[k]·H_k
    let lhs1 = g * z_a;
    let mut rhs1 = cap_a + cap_b * x;
    for k in 0..M {
        rhs1 -= hk[k] * f[k];
    }
    if lhs1.to_affine() != rhs1.to_affine() {
        return false;
    }

    // Check 2 — Bitness: f_k·(x - f_k) = j_k(1-j_k)·x² + ...
    //   z_c·g == x·C + D - Σ_k (f[k]*(x-f[k]))·H_k
    // Only holds if j_k ∈ {0,1} — by Schwartz-Zippel over random x.
    let lhs2 = g * z_c;
    let mut rhs2 = cap_c * x + cap_d;
    for k in 0..M {
        rhs2 -= hk[k] * (f[k] * (x - f[k]));
    }
    if lhs2.to_affine() != rhs2.to_affine() {
        return false;
    }

    // ── 4.5 Verify the Polynomial Identity ──────────────────────────────────
    // Membership check — O(N) group operations:
    //   Σ_j p_j(x)·D_j == z_g·g + Σ_{m≠l} z_m·h_m + z_name·h_name + Σ_k x^k·E[k]
    //
    // Where p_j(x) = Π_k f_{k,j_k}(x) with f_{k,1}=f_k and f_{k,0}=x-f_k.
    // By Schwartz-Zippel: if this holds for random x, the polynomial identity
    // holds formally, proving D_j is a commitment-to-zero at the prover's index.
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

    // RHS: z_g·g + Σ_{m≠l} z_m·h_m + z_name·h_name + Σ_k x^k·E[k]
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

    // z_name · h_name
    let z_name = scalar_from_bytes(&proof.z_responses[resp_idx]);
    rhs3 += crs.h_name * z_name;

    // Σ_k x^k · E[k]
    let mut x_pow = Scalar::ONE;
    for k in 0..M {
        rhs3 += cap_e[k] * x_pow;
        x_pow = x_pow * x;
    }

    if lhs3.to_affine() != rhs3.to_affine() {
        return false;
    }

    // ── 4.6 Verify Nullifier Correctness ────────────────────────────────────
    // Direct check: nul_l = s_l · g
    let expected_nul = point_to_bytes(&(g * s_l));
    if expected_nul != *public_nullifier {
        return false;
    }

    // Schnorr π_value: proves knowledge of s_l such that nul_l = s_l · g.
    // Verify: s·g == R + e·nul_l
    let schnorr_r = match point_from_bytes(&proof.schnorr_r) { Some(p) => p, None => return false };
    let schnorr_s = scalar_from_bytes(&proof.schnorr_s);
    let schnorr_e = schnorr_challenge(&g, public_nullifier, &schnorr_r);
    let nul_point = match point_from_bytes(public_nullifier) { Some(p) => p, None => return false };

    let schnorr_lhs = g * schnorr_s;
    let schnorr_rhs = schnorr_r + nul_point * schnorr_e;
    schnorr_lhs.to_affine() == schnorr_rhs.to_affine()
}

// ── proof serialization ─────────────────────────────────────────────────────
//
// Fixed portion size = 4×33 + M×33 + M×32 + 2×32 + 33 + 3×32 = PROOF_BASE_SIZE.
// Variable portion: (L+1) × 32 bytes (z_responses).
// Total: PROOF_BASE_SIZE + (L+1)×32.

const PROOF_BASE_SIZE: usize = 4 * 33 + M * 33 + M * 32 + 2 * 32 + 33 + 3 * 32;
const EPOLY_START: usize = 4 * 33;                         // after a,b,c,d
const F_START: usize = EPOLY_START + M * 33;               // after e_poly
const ZA_START: usize = F_START + M * 32;                  // after f
const ZC_START: usize = ZA_START + 32;
const SCHNORR_R_START: usize = ZC_START + 32;
const SCHNORR_S_START: usize = SCHNORR_R_START + 33;
const NULLIFIER_SCALAR_START: usize = SCHNORR_S_START + 32;
const NAME_SCALAR_START: usize = NULLIFIER_SCALAR_START + 32;

/// Serialize a `ServiceRegistrationProof` to bytes.
pub fn serialize_payment_identity_registration_proof(proof: &PaymentIdentityRegistrationProof) -> Vec<u8> {
    let z_len = proof.z_responses.len() * 32;
    let mut buf = Vec::with_capacity(PROOF_BASE_SIZE + z_len);

    buf.extend_from_slice(&proof.a);
    buf.extend_from_slice(&proof.b);
    buf.extend_from_slice(&proof.c);
    buf.extend_from_slice(&proof.d);
    for e in &proof.e_poly {
        buf.extend_from_slice(e);
    }
    for fi in &proof.f {
        buf.extend_from_slice(fi);
    }
    buf.extend_from_slice(&proof.z_a);
    buf.extend_from_slice(&proof.z_c);
    buf.extend_from_slice(&proof.schnorr_r);
    buf.extend_from_slice(&proof.schnorr_s);
    buf.extend_from_slice(&proof.nullifier_scalar);
    buf.extend_from_slice(&proof.name_scalar);
    for z in &proof.z_responses {
        buf.extend_from_slice(z);
    }

    buf
}

/// Deserialize a `ServiceRegistrationProof` from bytes.
///
/// The variable-length `z_responses` portion must be a multiple of 32 bytes.
pub fn deserialize_payment_identity_registration_proof(b: &[u8]) -> Result<PaymentIdentityRegistrationProof, String> {
    if b.len() < PROOF_BASE_SIZE {
        return Err(format!("proof too short: {} bytes (minimum {})", b.len(), PROOF_BASE_SIZE));
    }
    let z_bytes = b.len() - PROOF_BASE_SIZE;
    if z_bytes % 32 != 0 {
        return Err(format!("z_responses portion ({z_bytes} bytes) not a multiple of 32"));
    }

    let mut e_poly = [[0u8; 33]; M];
    for (k, slot) in e_poly.iter_mut().enumerate() {
        let start = EPOLY_START + k * 33;
        slot.copy_from_slice(&b[start..start + 33]);
    }

    let mut f = [[0u8; 32]; M];
    for (k, slot) in f.iter_mut().enumerate() {
        let start = F_START + k * 32;
        slot.copy_from_slice(&b[start..start + 32]);
    }

    let z_count = z_bytes / 32;
    let mut z_responses = Vec::with_capacity(z_count);
    for i in 0..z_count {
        let start = PROOF_BASE_SIZE + i * 32;
        let mut z = [0u8; 32];
        z.copy_from_slice(&b[start..start + 32]);
        z_responses.push(z);
    }

    Ok(PaymentIdentityRegistrationProof {
        a:  b[  0.. 33].try_into().unwrap(),
        b:  b[ 33.. 66].try_into().unwrap(),
        c:  b[ 66.. 99].try_into().unwrap(),
        d:  b[ 99..132].try_into().unwrap(),
        e_poly,
        f,
        z_a: b[ZA_START..ZC_START].try_into().unwrap(),
        z_c: b[ZC_START..SCHNORR_R_START].try_into().unwrap(),
        schnorr_r: b[SCHNORR_R_START..SCHNORR_S_START].try_into().unwrap(),
        schnorr_s: b[SCHNORR_S_START..NULLIFIER_SCALAR_START].try_into().unwrap(),
        nullifier_scalar: b[NULLIFIER_SCALAR_START..NAME_SCALAR_START].try_into().unwrap(),
        name_scalar: b[NAME_SCALAR_START..PROOF_BASE_SIZE].try_into().unwrap(),
        z_responses,
    })
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::request::derive_payment_request_pseudonym;
    use crate::core::credential::MasterCredential;
    use crate::core::merchant::Merchant;
    use crate::core::nullifier::derive_public_nullifier;
    use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret, Name};

    fn make_provider(name: &str) -> Merchant {
        Merchant::new(name, &format!("https://{name}"))
    }

    fn make_crs(n: usize) -> Crs {
        let merchants: Vec<Merchant> = (0..n)
            .map(|i| make_provider(&format!("user-{i}")))
            .collect();
        Crs::setup(merchants, N)
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        let sk = MasterSecret([seed; 32]);
        let r = ChildRandomness([seed.wrapping_add(1); 32]);
        let k = BlindingKey([seed.wrapping_add(2); 32]);
        let name = FriendlyName::new(format!("user-{seed:02x}"));
        MasterCredential::create(crs, sk, r, k, name)
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
        let target_pos = 5;
        let (cred, set) = make_full_set(&crs, 0xAA, target_pos);

        let user_index = 2; // 1-indexed
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_payment_request_pseudonym(&cred.r, &crs.merchants[user_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, user_index);

        let proof = prove_payment_identity_registration(
            &crs,
            &set,
            target_pos,
            user_index,
            TEST_SET_ID,
            &cred.k.0,
            &all_nullifiers,
            &pseudonym,
            &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .expect("proof generation should succeed");

        assert!(
            verify_payment_identity_registration_proof(
                &crs,
                &set,
                user_index,
                TEST_SET_ID,
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
        let (cred, set) = make_full_set(&crs, 0xBB, 3);

        let user_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_payment_request_pseudonym(&cred.r, &crs.merchants[user_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, user_index);

        let proof = prove_payment_identity_registration(
            &crs, &set, 3, user_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .unwrap();

        // The nullifier_scalar is now embedded in the proof.
        // Tamper with it to simulate a wrong nullifier scalar.
        let mut tampered_proof = proof;
        tampered_proof.nullifier_scalar = [0xFF; 32];
        let wrong_pub = derive_public_nullifier(
            &MasterSecret([0xFF; 32]),
            &crs.merchants[0].name,
            &crs.g,
        );
        assert!(!verify_payment_identity_registration_proof(
            &crs, &set, user_index, TEST_SET_ID,
            &pseudonym, &wrong_pub, &tampered_proof,
        ));
    }

    #[test]
    fn wrong_service_index_fails() {
        let crs = make_crs(3);
        let (cred, set) = make_full_set(&crs, 0xCC, 4);

        let user_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_payment_request_pseudonym(&cred.r, &crs.merchants[user_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, user_index);

        let proof = prove_payment_identity_registration(
            &crs, &set, 4, user_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .unwrap();

        // Verify at different service index — should fail
        let other_service = 2;
        let other_pub_nul = cred.public_nullifier(&crs, other_service);
        assert!(!verify_payment_identity_registration_proof(
            &crs, &set, other_service, TEST_SET_ID,
            &pseudonym, &other_pub_nul, &proof,
        ));
    }

    #[test]
    fn tampered_proof_fails() {
        let crs = make_crs(2);
        let (cred, set) = make_full_set(&crs, 0xDD, 6);

        let user_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_payment_request_pseudonym(&cred.r, &crs.merchants[user_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, user_index);

        let mut proof = prove_payment_identity_registration(
            &crs, &set, 6, user_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .unwrap();

        // Flip a byte in z_responses
        proof.z_responses[0][0] ^= 0xFF;

        assert!(!verify_payment_identity_registration_proof(
            &crs, &set, user_index, TEST_SET_ID,
            &pseudonym, &pub_nul, &proof,
        ));
    }

    #[test]
    fn proof_size_matches_expected() {
        let l_count = 4;
        let crs = make_crs(l_count);
        let (cred, set) = make_full_set(&crs, 0xEE, 0);

        let user_index = 1;
        let all_nullifiers = cred.all_nullifier_scalars(&crs);
        let pseudonym = derive_payment_request_pseudonym(&cred.r, &crs.merchants[user_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(&crs, user_index);

        let proof = prove_payment_identity_registration(
            &crs, &set, 0, user_index, TEST_SET_ID,
            &cred.k.0, &all_nullifiers, &pseudonym, &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .unwrap();

        // Fixed portion: 4×33 (ABCD) + M×33 (E_poly) + M×32 (f) + 2×32 (z_a, z_c)
        //   + 33 (schnorr_r) + 3×32 (schnorr_s, nullifier_scalar, name_scalar)
        //   = PROOF_BASE_SIZE
        // Variable: (L+1)×32 (z_responses: g + L-1 h_{m≠l} + h_name)
        // Total: PROOF_BASE_SIZE + (L+1)×32
        let expected = PROOF_BASE_SIZE + (l_count + 1) * 32;
        let actual = 4 * 33 + M * 33 + M * 32 + 2 * 32
            + proof.z_responses.len() * 32
            + 33 + 32  // schnorr_r + schnorr_s
            + 32       // nullifier_scalar
            + 32;      // name_scalar
        assert_eq!(actual, expected);
        assert_eq!(proof.z_responses.len(), l_count + 1);
    }
}
