//! Master credential creation (Phase 1) and registration (Phase 2).
//!
//! Phase 1 — local, offline:
//! ```text
//! sk, r, k  ←$ random
//! for l = 1..L:
//!   s_l = HKDF(sk, v_l)
//!   nul_l = s_l · g
//! Φ = k·g + s_1·h_1 + ... + s_L·h_L    (33 bytes)
//! store (sk, r, k) securely
//! ```
//!
//! Phase 2 — on-chain registration:
//! ```text
//! send Φ to IdR (Bitcoin)
//! wait for Λ_{d̂} to fill to N=1024
//! receive Λ_{d̂} = [Φ_1, ..., Φ_1024]
//! determine own index j
//! ```

use crate::crs::Crs;
use crate::nullifier_v2::{derive_all_nullifiers, derive_nullifier, derive_public_nullifier};
use crate::types::{BlindingKey, ChildRandomness, Commitment, MasterSecret, Nullifier};

/// The master credential tuple `(Φ, sk, r, k)`.
///
/// Created locally during Phase 1. The user only needs to store `(sk, r, k)`
/// — about 96 bytes — since `Φ` can be recomputed on demand from these
/// values and the CRS.
#[derive(Debug, Clone)]
pub struct MasterCredential {
    /// Public master identity Φ (33-byte compressed secp256k1 point).
    /// Posted to the Identity Registry.
    pub phi: Commitment,
    /// Root secret for nullifier derivation via HKDF.
    pub sk: MasterSecret,
    /// Child credential randomness for deriving service-specific auth keys.
    pub r: ChildRandomness,
    /// Pedersen blinding key (hides the commitment).
    pub k: BlindingKey,
}

impl MasterCredential {
    /// Create a new master credential from random secrets (Phase 1).
    ///
    /// Performs the full local credential creation:
    /// 1. Derives all L nullifier scalars `s_l = HKDF(sk, v_l)`
    /// 2. Computes `Φ = k·g + s_1·h_1 + ... + s_L·h_L`
    ///
    /// The caller is responsible for generating `sk`, `r`, `k` from a
    /// cryptographically secure random source.
    pub fn create(crs: &Crs, sk: MasterSecret, r: ChildRandomness, k: BlindingKey) -> Self {
        let names = crs.service_names();
        let nullifiers = derive_all_nullifiers(&sk, &names);
        let phi = crs
            .commit_master_identity(&nullifiers, &k)
            .expect("nullifier count matches CRS providers");
        MasterCredential { phi, sk, r, k }
    }

    /// Derive the nullifier scalar `s_l` for service provider at index `l` (1-indexed).
    ///
    /// `s_l = HKDF(sk, v_l)` — the raw 32-byte scalar used in the commitment.
    pub fn nullifier_scalar(&self, crs: &Crs, l: usize) -> Nullifier {
        assert!(l >= 1 && l <= crs.num_providers(), "provider index out of range");
        derive_nullifier(&self.sk, &crs.providers[l - 1].name)
    }

    /// Derive ALL L nullifier scalars.
    pub fn all_nullifier_scalars(&self, crs: &Crs) -> Vec<Nullifier> {
        derive_all_nullifiers(&self.sk, &crs.service_names())
    }

    /// Derive the public nullifier `nul_l = s_l · g` for service at index `l` (1-indexed).
    ///
    /// Returns a 33-byte compressed secp256k1 point.
    /// This serves as both a Sybil-resistance token and a public authentication key.
    pub fn public_nullifier(&self, crs: &Crs, l: usize) -> [u8; 33] {
        assert!(l >= 1 && l <= crs.num_providers(), "provider index out of range");
        derive_public_nullifier(&self.sk, &crs.providers[l - 1].name)
    }

    /// Recompute `Φ` from the stored secrets and the CRS.
    ///
    /// The user only needs to store `(sk, r, k)` — Φ can always be
    /// recomputed since `Φ = k·g + Σ s_l·h_l` and `s_l = HKDF(sk, v_l)`.
    pub fn recompute_phi(&self, crs: &Crs) -> Commitment {
        let names = crs.service_names();
        let nullifiers = derive_all_nullifiers(&self.sk, &names);
        crs.commit_master_identity(&nullifiers, &self.k)
            .expect("nullifier count matches CRS providers")
    }
}

/// A registered master credential — the user's full state after Phase 2.
///
/// After registration and the anonymity set filling to N=1024, the user
/// stores this locally. The frozen anonymity set is needed for generating
/// Bootle/Groth membership proofs in Phase 3.
///
/// ```text
/// (Φ_j, sk, r, k, d̂, Λ_{d̂})
/// ```
#[derive(Debug, Clone)]
pub struct RegisteredIdentity {
    /// The master credential `(Φ, sk, r, k)`.
    pub credential: MasterCredential,
    /// `d̂` — which anonymity set the user is in.
    pub set_id: u64,
    /// `j` — the user's index within `Λ_{d̂}` (0-based).
    pub index: usize,
    /// `Λ_{d̂}` — the complete frozen anonymity set `[Φ_1, ..., Φ_N]`.
    pub anonymity_set: Vec<Commitment>,
}

impl RegisteredIdentity {
    /// Construct a `RegisteredIdentity` after Phase 2 completes.
    ///
    /// Finds the user's index `j` by searching for their `Φ` in the set.
    /// Returns an error if `Φ` is not found in the anonymity set.
    pub fn new(
        credential: MasterCredential,
        set_id: u64,
        anonymity_set: Vec<Commitment>,
    ) -> Result<Self, &'static str> {
        let index = Self::determine_index(&credential, &anonymity_set)
            .ok_or("master identity not found in anonymity set")?;
        Ok(Self {
            credential,
            set_id,
            index,
            anonymity_set,
        })
    }

    /// Find the user's index `j` by searching for `Φ` in the anonymity set.
    pub fn determine_index(credential: &MasterCredential, set: &[Commitment]) -> Option<usize> {
        set.iter().position(|c| *c == credential.phi)
    }

    /// Returns the number of identities in the anonymity set.
    pub fn set_size(&self) -> usize {
        self.anonymity_set.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crs::ServiceProvider;
    use crate::nullifier_v2::derive_all_public_nullifiers;
    use crate::types::Name;

    fn make_provider(name: &str) -> ServiceProvider {
        ServiceProvider {
            name: Name::new(name),
            credential_generator: [0x02; 33],
            origin: format!("https://{name}"),
        }
    }

    fn make_crs(n: usize) -> Crs {
        let providers: Vec<ServiceProvider> = (0..n)
            .map(|i| make_provider(&format!("service-{i}")))
            .collect();
        Crs::setup(providers)
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        let sk = MasterSecret([seed; 32]);
        let r = ChildRandomness([seed.wrapping_add(1); 32]);
        let k = BlindingKey([seed.wrapping_add(2); 32]);
        MasterCredential::create(crs, sk, r, k)
    }

    // ── Phase 1 tests ───────────────────────────────────────────────────────

    #[test]
    fn create_produces_valid_commitment() {
        let crs = make_crs(4);
        let cred = make_credential(&crs, 0x42);
        assert_eq!(cred.phi.as_bytes().len(), 33);
        assert!(cred.phi.as_bytes()[0] == 0x02 || cred.phi.as_bytes()[0] == 0x03);
    }

    #[test]
    fn recompute_phi_matches_original() {
        let crs = make_crs(4);
        let cred = make_credential(&crs, 0x42);
        assert_eq!(cred.recompute_phi(&crs), cred.phi);
    }

    #[test]
    fn different_secrets_produce_different_phi() {
        let crs = make_crs(4);
        let c1 = make_credential(&crs, 0x01);
        let c2 = make_credential(&crs, 0x02);
        assert_ne!(c1.phi, c2.phi);
    }

    #[test]
    fn nullifier_scalar_matches_derive() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0x42);
        let names = crs.service_names();
        for l in 1..=3 {
            let from_cred = cred.nullifier_scalar(&crs, l);
            let from_derive = derive_nullifier(&cred.sk, &names[l - 1]);
            assert_eq!(from_cred, from_derive);
        }
    }

    #[test]
    fn all_nullifier_scalars_correct_count() {
        let crs = make_crs(5);
        let cred = make_credential(&crs, 0x42);
        let scalars = cred.all_nullifier_scalars(&crs);
        assert_eq!(scalars.len(), 5);
    }

    #[test]
    fn public_nullifier_matches_derive() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0x42);
        let names = crs.service_names();
        for l in 1..=3 {
            let from_cred = cred.public_nullifier(&crs, l);
            let from_derive = derive_public_nullifier(&cred.sk, &names[l - 1]);
            assert_eq!(from_cred, from_derive);
        }
    }

    #[test]
    fn public_nullifiers_are_all_different() {
        let crs = make_crs(5);
        let cred = make_credential(&crs, 0x42);
        let pns: Vec<[u8; 33]> = (1..=5).map(|l| cred.public_nullifier(&crs, l)).collect();
        for i in 0..pns.len() {
            for j in (i + 1)..pns.len() {
                assert_ne!(pns[i], pns[j]);
            }
        }
    }

    // ── Phase 2 tests ───────────────────────────────────────────────────────

    #[test]
    fn determine_index_finds_own_phi() {
        let crs = make_crs(3);
        let target = make_credential(&crs, 0x42);
        // Build a set with the target at position 2
        let mut set = Vec::new();
        set.push(make_credential(&crs, 0x01).phi);
        set.push(make_credential(&crs, 0x02).phi);
        set.push(target.phi);
        set.push(make_credential(&crs, 0x03).phi);

        assert_eq!(RegisteredIdentity::determine_index(&target, &set), Some(2));
    }

    #[test]
    fn determine_index_returns_none_if_not_found() {
        let crs = make_crs(3);
        let target = make_credential(&crs, 0x42);
        let set = vec![
            make_credential(&crs, 0x01).phi,
            make_credential(&crs, 0x02).phi,
        ];
        assert_eq!(RegisteredIdentity::determine_index(&target, &set), None);
    }

    #[test]
    fn registered_identity_new_succeeds() {
        let crs = make_crs(3);
        let target = make_credential(&crs, 0x42);
        let set = vec![
            make_credential(&crs, 0x01).phi,
            target.phi,
            make_credential(&crs, 0x02).phi,
            make_credential(&crs, 0x03).phi,
        ];
        let reg = RegisteredIdentity::new(target, 0, set).unwrap();
        assert_eq!(reg.index, 1);
        assert_eq!(reg.set_id, 0);
        assert_eq!(reg.set_size(), 4);
    }

    #[test]
    fn registered_identity_new_fails_if_not_in_set() {
        let crs = make_crs(3);
        let target = make_credential(&crs, 0x42);
        let set = vec![make_credential(&crs, 0x01).phi, make_credential(&crs, 0x02).phi];
        assert!(RegisteredIdentity::new(target, 0, set).is_err());
    }

    #[test]
    fn full_phase_1_and_2_flow() {
        // Phase 0: Setup CRS with 4 service providers
        let crs = make_crs(4);

        // Phase 1: Create master credential (local, offline)
        let my_cred = make_credential(&crs, 0xAA);

        // Verify all nullifier scalars are unique
        let scalars = my_cred.all_nullifier_scalars(&crs);
        let unique: std::collections::HashSet<_> = scalars.iter().map(|n| n.0).collect();
        assert_eq!(unique.len(), 4);

        // Verify public nullifiers are valid points
        let names = crs.service_names();
        let pub_nuls = derive_all_public_nullifiers(&my_cred.sk, &names);
        for pn in &pub_nuls {
            assert!(pn[0] == 0x02 || pn[0] == 0x03);
        }

        // Verify Φ is reproducible
        assert_eq!(my_cred.recompute_phi(&crs), my_cred.phi);

        // Phase 2: Register + fill anonymity set
        // Simulate: my_cred is one of 8 users (power of 2 for vtxo-tree)
        let mut anonymity_set = Vec::new();
        for seed in 1..=7u8 {
            anonymity_set.push(make_credential(&crs, seed).phi);
        }
        // Insert our credential at position 3
        anonymity_set.insert(3, my_cred.phi);
        assert_eq!(anonymity_set.len(), 8);

        // Determine own index
        let reg = RegisteredIdentity::new(my_cred, 0, anonymity_set).unwrap();
        assert_eq!(reg.index, 3);
        assert_eq!(reg.set_size(), 8);

        // Verify the commitment at our index matches
        assert_eq!(reg.anonymity_set[reg.index], reg.credential.phi);
    }
}
