//! Verifier-side proof verification (Phase 4).
//!
//! When Bob (user `l`) receives `(ϕ, nul_l, π, d̂)` from Alice, he runs
//! steps 4.1–4.8 to verify her membership in a sealed anonymity set and
//! register her pseudonym.
//!
//! Steps:
//! ```text
//! 4.1  Fetch Λ_{d̂} (anonymity set) — from cache or registry
//! 4.2  Recompute D_1..D_N (shifted commitments)
//! 4.3  Recompute Fiat-Shamir challenge x
//! 4.4  Verify 10 × bitness equations
//! 4.5  Verify polynomial identity (O(N) group ops — Schwartz-Zippel)
//! 4.6  Verify nullifier correctness (Schnorr π_value)
//! 4.7  Check nul_l ∉ nullifier list, ϕ ∉ pseudonym list
//! 4.8  Store (ϕ, nul_l), return "Registered"
//! ```

use std::collections::{HashMap, HashSet};

use crate::core::crs::Crs;
use crate::core::service_proof::{verify_service_registration, ServiceRegistrationProof};
use crate::core::types::Commitment;

// ── error types ─────────────────────────────────────────────────────────────

/// Errors during verification (steps 4.1–4.8).
#[derive(Debug, PartialEq, Eq)]
pub enum VerificationError {
    /// 4.1: anonymity set not cached/fetchable.
    SetNotFound(u64),
    /// 4.2–4.6: cryptographic check failed.
    ProofInvalid,
    /// 4.7: Sybil attempt — same identity already registered.
    NullifierAlreadyUsed,
    /// 4.7: pseudonym already registered.
    PseudonymAlreadyUsed,
}

/// Result of a successful verification + registration (step 4.8).
#[derive(Debug, PartialEq, Eq)]
pub struct RegistrationResult {
    pub pseudonym: [u8; 33],
    pub public_nullifier: [u8; 33],
}

// ── verifier state ──────────────────────────────────────────────────────────

/// Verifier state for a user acting as verifier (Bob's side).
///
/// Bob is user `l` in the CRS. When Alice proves membership and presents
/// her nullifier `nul_l = s_l · g`, Bob verifies the proof and stores
/// `(ϕ, nul_l)` to track registered pseudonyms and prevent Sybil attacks.
pub struct VerifierState {
    /// This user's index in the CRS (1-indexed).
    pub user_index: usize,
    /// Cached frozen anonymity sets — keyed by set_id.
    /// Frozen sets never change and can be cached indefinitely (step 4.1).
    set_cache: HashMap<u64, Vec<Commitment>>,
    /// Registered nullifiers — for Sybil resistance (step 4.7).
    registered_nullifiers: HashSet<[u8; 33]>,
    /// Registered pseudonyms — for duplicate detection (step 4.7).
    registered_pseudonyms: HashSet<[u8; 33]>,
}

impl VerifierState {
    /// Create a new empty verifier state.
    ///
    /// `user_index` is 1-indexed — this user's position in the CRS.
    pub fn new(user_index: usize) -> Self {
        Self {
            user_index,
            set_cache: HashMap::new(),
            registered_nullifiers: HashSet::new(),
            registered_pseudonyms: HashSet::new(),
        }
    }

    /// Cache a frozen anonymity set (step 4.1).
    ///
    /// Frozen sets are immutable once sealed, so caching is safe.
    pub fn cache_set(&mut self, set_id: u64, commitments: Vec<Commitment>) {
        self.set_cache.insert(set_id, commitments);
    }

    /// Look up a cached anonymity set (step 4.1).
    pub fn get_cached_set(&self, set_id: u64) -> Option<&Vec<Commitment>> {
        self.set_cache.get(&set_id)
    }

    /// Check whether a nullifier has already been registered.
    pub fn has_nullifier(&self, public_nullifier: &[u8; 33]) -> bool {
        self.registered_nullifiers.contains(public_nullifier)
    }

    /// Check whether a pseudonym has already been registered.
    pub fn has_pseudonym(&self, pseudonym: &[u8; 33]) -> bool {
        self.registered_pseudonyms.contains(pseudonym)
    }

    /// Return the number of registered pseudonyms.
    pub fn registered_count(&self) -> usize {
        self.registered_pseudonyms.len()
    }

    /// Steps 4.7: Check freshness of nullifier and pseudonym.
    fn check_freshness(
        &self,
        pseudonym: &[u8; 33],
        public_nullifier: &[u8; 33],
    ) -> Result<(), VerificationError> {
        if self.registered_nullifiers.contains(public_nullifier) {
            return Err(VerificationError::NullifierAlreadyUsed);
        }
        if self.registered_pseudonyms.contains(pseudonym) {
            return Err(VerificationError::PseudonymAlreadyUsed);
        }
        Ok(())
    }

    /// Steps 4.1–4.8: Verify a proof and register the pseudonym.
    ///
    /// Bob receives `(ϕ, nul_l, π, d̂)` from Alice:
    ///
    /// - 4.1  Fetch `Λ_{d̂}` from cache
    /// - 4.2  Recompute `D_1..D_N` (inside `verify_service_registration`)
    /// - 4.3  Recompute Fiat-Shamir challenge `x`
    /// - 4.4  Verify 10 × bitness equations
    /// - 4.5  Verify polynomial identity (O(N) group ops)
    /// - 4.6  Verify nullifier correctness (Schnorr π_value)
    /// - 4.7  Check `nul_l` not in nullifier list, `ϕ` not in pseudonym list
    /// - 4.8  Store `(ϕ, nul_l)`, return "Registered"
    pub fn verify_and_register(
        &mut self,
        crs: &Crs,
        pseudonym: &[u8; 33],
        public_nullifier: &[u8; 33],
        proof: &ServiceRegistrationProof,
        set_id: u64,
    ) -> Result<RegistrationResult, VerificationError> {
        // 4.1: Fetch anonymity set from cache.
        let anonymity_set = self
            .set_cache
            .get(&set_id)
            .ok_or(VerificationError::SetNotFound(set_id))?;

        // 4.2–4.6: Cryptographic verification (shifted commitments, Fiat-Shamir,
        // bitness, polynomial identity, nullifier correctness).
        let valid = verify_service_registration(
            crs,
            anonymity_set,
            self.user_index,
            set_id,
            pseudonym,
            public_nullifier,
            proof,
        );
        if !valid {
            return Err(VerificationError::ProofInvalid);
        }

        // 4.7: Check freshness — no duplicate nullifiers or pseudonyms.
        self.check_freshness(pseudonym, public_nullifier)?;

        // 4.8: Store (ϕ, nul_l) and return "Registered".
        self.registered_nullifiers.insert(*public_nullifier);
        self.registered_pseudonyms.insert(*pseudonym);

        Ok(RegistrationResult {
            pseudonym: *pseudonym,
            public_nullifier: *public_nullifier,
        })
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::child_credential::derive_pseudonym;
    use crate::core::credential::MasterCredential;
    use crate::core::crs::User;
    use crate::core::service_proof::prove_service_registration;
    use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret, Name};

    fn make_provider(name: &str) -> User {
        User {
            name: Name::new(name),
            credential_generator: [0x02; 33],
            origin: format!("https://{name}"),
        }
    }

    fn make_crs(n: usize) -> Crs {
        let providers: Vec<User> = (0..n)
            .map(|i| make_provider(&format!("user-{i}")))
            .collect();
        Crs::setup(providers)
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        let sk = MasterSecret([seed; 32]);
        let r = ChildRandomness([seed.wrapping_add(1); 32]);
        let k = BlindingKey([seed.wrapping_add(2); 32]);
        let name = FriendlyName::new(format!("user-{seed:02x}"));
        MasterCredential::create(crs, sk, r, k, name)
    }

    const N: usize = 1024;
    const TEST_SET_ID: u64 = 7;

    fn make_full_set(crs: &Crs, target_seed: u8, target_pos: usize) -> (MasterCredential, Vec<Commitment>) {
        let target = make_credential(crs, target_seed);
        let mut set = Vec::with_capacity(N);
        for i in 0..N {
            if i == target_pos {
                set.push(target.phi);
            } else {
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

    /// Helper: generate a valid proof for user at `target_pos` registering with `service_index`.
    fn make_valid_proof(
        crs: &Crs,
        cred: &MasterCredential,
        set: &[Commitment],
        target_pos: usize,
        service_index: usize,
        set_id: u64,
    ) -> (ServiceRegistrationProof, [u8; 33], [u8; 33]) {
        let all_nullifiers = cred.all_nullifier_scalars(crs);
        let pseudonym = derive_pseudonym(&cred.r, &crs.providers[service_index - 1].name, &crs.g);
        let pub_nul = cred.public_nullifier(crs, service_index);

        let proof = prove_service_registration(
            crs,
            set,
            target_pos,
            service_index,
            set_id,
            &cred.k.0,
            &all_nullifiers,
            &pseudonym,
            &pub_nul,
            &cred.friendly_name.to_scalar_bytes(),
        )
        .expect("proof generation should succeed");

        (proof, pseudonym, pub_nul)
    }

    // ── 1. new_verifier_state ───────────────────────────────────────────────

    #[test]
    fn new_verifier_state() {
        let vs = VerifierState::new(1);
        assert_eq!(vs.user_index, 1);
        assert_eq!(vs.registered_count(), 0);
        assert!(vs.get_cached_set(0).is_none());
    }

    // ── 2. cache_and_retrieve_set ───────────────────────────────────────────

    #[test]
    fn cache_and_retrieve_set() {
        let mut vs = VerifierState::new(1);
        let dummy = vec![Commitment([0x02; 33]), Commitment([0x03; 33])];
        vs.cache_set(42, dummy.clone());
        let retrieved = vs.get_cached_set(42).unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0], dummy[0]);
        assert!(vs.get_cached_set(99).is_none());
    }

    // ── 3. freshness_passes_for_new_values ──────────────────────────────────

    #[test]
    fn freshness_passes_for_new_values() {
        let vs = VerifierState::new(1);
        let nul = [0x02; 33];
        let pseudo = [0x03; 33];
        assert!(vs.check_freshness(&pseudo, &nul).is_ok());
    }

    // ── 4. freshness_fails_duplicate_nullifier ──────────────────────────────

    #[test]
    fn freshness_fails_duplicate_nullifier() {
        let mut vs = VerifierState::new(1);
        let nul = [0x02; 33];
        let pseudo = [0x03; 33];
        vs.registered_nullifiers.insert(nul);
        assert_eq!(
            vs.check_freshness(&pseudo, &nul),
            Err(VerificationError::NullifierAlreadyUsed)
        );
    }

    // ── 5. freshness_fails_duplicate_pseudonym ──────────────────────────────

    #[test]
    fn freshness_fails_duplicate_pseudonym() {
        let mut vs = VerifierState::new(1);
        let nul = [0x02; 33];
        let pseudo = [0x03; 33];
        vs.registered_pseudonyms.insert(pseudo);
        assert_eq!(
            vs.check_freshness(&pseudo, &nul),
            Err(VerificationError::PseudonymAlreadyUsed)
        );
    }

    // ── 6. verify_and_register_full_flow ────────────────────────────────────

    #[test]
    fn verify_and_register_full_flow() {
        let crs = make_crs(3);
        let service_index = 2; // Bob is user 2
        let target_pos = 42;

        let (cred, set) = make_full_set(&crs, 0xAA, target_pos);
        let (proof, pseudonym, pub_nul) =
            make_valid_proof(&crs, &cred, &set, target_pos, service_index, TEST_SET_ID);

        let mut vs = VerifierState::new(service_index);
        vs.cache_set(TEST_SET_ID, set);

        let result = vs
            .verify_and_register(&crs, &pseudonym, &pub_nul, &proof, TEST_SET_ID)
            .expect("valid proof should register");

        assert_eq!(result.pseudonym, pseudonym);
        assert_eq!(result.public_nullifier, pub_nul);
        assert_eq!(vs.registered_count(), 1);
        assert!(vs.has_nullifier(&pub_nul));
        assert!(vs.has_pseudonym(&pseudonym));
    }

    // ── 7. verify_and_register_rejects_replay ───────────────────────────────

    #[test]
    fn verify_and_register_rejects_replay() {
        let crs = make_crs(3);
        let service_index = 2;
        let target_pos = 42;

        let (cred, set) = make_full_set(&crs, 0xAA, target_pos);
        let (proof, pseudonym, pub_nul) =
            make_valid_proof(&crs, &cred, &set, target_pos, service_index, TEST_SET_ID);

        let mut vs = VerifierState::new(service_index);
        vs.cache_set(TEST_SET_ID, set);

        // First registration succeeds.
        assert!(vs.verify_and_register(&crs, &pseudonym, &pub_nul, &proof, TEST_SET_ID).is_ok());

        // Same proof again — nullifier already used.
        assert_eq!(
            vs.verify_and_register(&crs, &pseudonym, &pub_nul, &proof, TEST_SET_ID),
            Err(VerificationError::NullifierAlreadyUsed)
        );
    }

    // ── 8. verify_and_register_set_not_found ────────────────────────────────

    #[test]
    fn verify_and_register_set_not_found() {
        let crs = make_crs(3);
        let service_index = 1;
        let (cred, set) = make_full_set(&crs, 0xBB, 10);
        let (proof, pseudonym, pub_nul) =
            make_valid_proof(&crs, &cred, &set, 10, service_index, 99);

        let mut vs = VerifierState::new(service_index);
        // Do NOT cache set 99.

        assert_eq!(
            vs.verify_and_register(&crs, &pseudonym, &pub_nul, &proof, 99),
            Err(VerificationError::SetNotFound(99))
        );
    }

    // ── 9. verify_and_register_invalid_proof ────────────────────────────────

    #[test]
    fn verify_and_register_invalid_proof() {
        let crs = make_crs(3);
        let service_index = 1;
        let target_pos = 10;

        let (cred, set) = make_full_set(&crs, 0xCC, target_pos);
        let (mut proof, pseudonym, pub_nul) =
            make_valid_proof(&crs, &cred, &set, target_pos, service_index, TEST_SET_ID);

        // Tamper with the proof.
        proof.z_responses[0][0] ^= 0xFF;

        let mut vs = VerifierState::new(service_index);
        vs.cache_set(TEST_SET_ID, set);

        assert_eq!(
            vs.verify_and_register(&crs, &pseudonym, &pub_nul, &proof, TEST_SET_ID),
            Err(VerificationError::ProofInvalid)
        );
    }

    // ── 10. verify_and_register_wrong_user ──────────────────────────────────

    #[test]
    fn verify_and_register_wrong_user() {
        let crs = make_crs(3);
        let target_pos = 50;

        let (cred, set) = make_full_set(&crs, 0xDD, target_pos);
        // Proof generated for user 2.
        let (proof, pseudonym, pub_nul) =
            make_valid_proof(&crs, &cred, &set, target_pos, 2, TEST_SET_ID);

        // Verifier claims to be user 1 — mismatch.
        let mut vs = VerifierState::new(1);
        vs.cache_set(TEST_SET_ID, set);

        assert_eq!(
            vs.verify_and_register(&crs, &pseudonym, &pub_nul, &proof, TEST_SET_ID),
            Err(VerificationError::ProofInvalid)
        );
    }
}
