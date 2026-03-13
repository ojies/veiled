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

use hkdf::Hkdf;
use sha2::Sha256;

use crate::core::crs::Crs;
use crate::core::nullifier::{derive_all_nullifiers, derive_nullifier, derive_public_nullifier};
use crate::core::types::{BlindingKey, ChildRandomness, Commitment, FriendlyName, MasterSecret, Name, Nullifier};

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
    /// User-chosen global friendly name, committed inside Φ via `name_scalar · h_name`.
    pub friendly_name: FriendlyName,
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
    pub fn create(
        crs: &Crs,
        sk: MasterSecret,
        r: ChildRandomness,
        k: BlindingKey,
        friendly_name: FriendlyName,
    ) -> Self {
        let names = crs.names();
        let nullifiers = derive_all_nullifiers(&sk, &names);
        let name_scalar = friendly_name.to_scalar_bytes();
        let phi = crs
            .commit_master_identity(&nullifiers, &k, &name_scalar)
            .expect("nullifier count matches CRS users");
        MasterCredential { phi, sk, r, k, friendly_name }
    }

    /// Derive the nullifier scalar `s_l` for service provider at index `l` (1-indexed).
    ///
    /// `s_l = HKDF(sk, v_l)` — the raw 32-byte scalar used in the commitment.
    pub fn nullifier_scalar(&self, crs: &Crs, l: usize) -> Nullifier {
        assert!(l >= 1 && l <= crs.num_merchants(), "provider index out of range");
        derive_nullifier(&self.sk, &crs.merchants[l - 1].name)
    }

    /// Derive ALL L nullifier scalars.
    pub fn all_nullifier_scalars(&self, crs: &Crs) -> Vec<Nullifier> {
        derive_all_nullifiers(&self.sk, &crs.names())
    }

    /// Derive the public nullifier `nul_l = s_l · g` for service at index `l` (1-indexed).
    ///
    /// Returns a 33-byte compressed secp256k1 point.
    /// Uses the CRS base generator `g` (not the standard secp256k1 generator).
    /// This serves as both a Sybil-resistance token and a public authentication key.
    pub fn public_nullifier(&self, crs: &Crs, l: usize) -> [u8; 33] {
        assert!(l >= 1 && l <= crs.num_merchants(), "provider index out of range");
        derive_public_nullifier(&self.sk, &crs.merchants[l - 1].name, &crs.g)
    }

    /// Create a master credential with name-derived child randomness (Phase 1).
    ///
    /// `r = HKDF(IKM=real_randomness, salt=sha256(name), info="CRS-ASC-child-randomness")`
    ///
    /// This binds the child credential randomness to the user's own name,
    /// so that service-specific auth keys are deterministic given the same
    /// real randomness and name.
    pub fn create_with_name(
        crs: &Crs,
        sk: MasterSecret,
        real_randomness: &[u8; 32],
        name: &Name,
        k: BlindingKey,
        friendly_name: FriendlyName,
    ) -> Self {
        let r = derive_child_randomness(real_randomness, name);
        Self::create(crs, sk, r, k, friendly_name)
    }

    /// Recompute `Φ` from the stored secrets and the CRS.
    ///
    /// The user only needs to store `(sk, r, k)` — Φ can always be
    /// recomputed since `Φ = k·g + Σ s_l·h_l` and `s_l = HKDF(sk, v_l)`.
    pub fn recompute_phi(&self, crs: &Crs) -> Commitment {
        let names = crs.names();
        let nullifiers = derive_all_nullifiers(&self.sk, &names);
        let name_scalar = self.friendly_name.to_scalar_bytes();
        crs.commit_master_identity(&nullifiers, &self.k, &name_scalar)
            .expect("nullifier count matches CRS users")
    }
}

/// Derive child credential randomness from real randomness and a name.
///
/// ```text
/// r = HKDF(IKM = real_randomness, salt = sha256(name), info = "CRS-ASC-child-randomness")
/// ```
///
/// This combines the user's true randomness with their name so that
/// child credentials are deterministic for a given (randomness, name) pair.
pub fn derive_child_randomness(real_randomness: &[u8; 32], name: &Name) -> ChildRandomness {
    use sha2::Digest;
    let name_hash = Sha256::digest(name.as_str().as_bytes());
    let hk = Hkdf::<Sha256>::new(Some(&name_hash), real_randomness);
    let mut output = [0u8; 32];
    hk.expand(b"CRS-ASC-child-randomness", &mut output)
        .expect("32 bytes is valid for HKDF-SHA256");
    ChildRandomness(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::beneficiary::Beneficiary;
    use crate::core::merchant::Merchant;
    use crate::core::payment_identity::{PaymentIdentityRegistration, verify_payment_identity_registration};
    use crate::core::nullifier::derive_all_public_nullifiers;

    fn make_provider(name: &str) -> Merchant {
        Merchant::new(name, &format!("https://{name}"))
    }

    fn make_crs(n: usize) -> Crs {
        use crate::core::utils::N;
        let merchants: Vec<Merchant> = (0..n)
            .map(|i| make_provider(&format!("service-{i}")))
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

    /// Deterministic beneficiary for tests (bypasses random generation).
    fn make_beneficiary(crs: &Crs, seed: u8, friendly_name: &str) -> Beneficiary {
        let credential = MasterCredential::create(
            crs,
            MasterSecret([seed; 32]),
            ChildRandomness([seed.wrapping_add(1); 32]),
            BlindingKey([seed.wrapping_add(2); 32]),
            FriendlyName::new(friendly_name),
        );
        Beneficiary {
            credential,
            set_id: None,
            index: None,
            anonymity_set: None,
            registrations: std::collections::HashMap::new(),
        }
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
        let names = crs.names();
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
        let names = crs.names();
        for l in 1..=3 {
            let from_cred = cred.public_nullifier(&crs, l);
            let from_derive = derive_public_nullifier(&cred.sk, &names[l - 1], &crs.g);
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
    fn register_finds_own_phi() {
        let crs = make_crs(3);
        let mut target = make_beneficiary(&crs, 0x42, "target");
        let mut set = Vec::new();
        set.push(make_credential(&crs, 0x01).phi);
        set.push(make_credential(&crs, 0x02).phi);
        set.push(target.credential.phi);
        set.push(make_credential(&crs, 0x03).phi);

        target.register(0, set).unwrap();
        assert_eq!(target.index, Some(2));
    }

    #[test]
    fn register_fails_if_not_in_set() {
        let crs = make_crs(3);
        let mut target = make_beneficiary(&crs, 0x42, "target");
        let set = vec![
            make_credential(&crs, 0x01).phi,
            make_credential(&crs, 0x02).phi,
        ];
        assert!(target.register(0, set).is_err());
    }

    #[test]
    fn register_succeeds() {
        let crs = make_crs(3);
        let mut ben = make_beneficiary(&crs, 0x42, "target");
        let set = vec![
            make_credential(&crs, 0x01).phi,
            ben.credential.phi,
            make_credential(&crs, 0x02).phi,
            make_credential(&crs, 0x03).phi,
        ];
        ben.register(0, set).unwrap();
        assert_eq!(ben.index, Some(1));
        assert_eq!(ben.set_id, Some(0));
        assert_eq!(ben.set_size(), Some(4));
    }

    #[test]
    fn full_phase_1_and_2_flow() {
        // Phase 0: Setup CRS with 4 service users
        let crs = make_crs(4);

        // Phase 1: Create beneficiary (local, offline)
        let mut ben = make_beneficiary(&crs, 0xAA, "my-user");

        // Verify all nullifier scalars are unique
        let scalars = ben.credential.all_nullifier_scalars(&crs);
        let unique: std::collections::HashSet<_> = scalars.iter().map(|n| n.0).collect();
        assert_eq!(unique.len(), 4);

        // Verify public nullifiers are valid points
        let names = crs.names();
        let pub_nuls = derive_all_public_nullifiers(&ben.credential.sk, &names, &crs.g);
        for pn in &pub_nuls {
            assert!(pn[0] == 0x02 || pn[0] == 0x03);
        }

        // Verify Φ is reproducible
        assert_eq!(ben.credential.recompute_phi(&crs), ben.credential.phi);

        // Phase 2: Register + fill anonymity set
        let mut anonymity_set = Vec::new();
        for seed in 1..=7u8 {
            anonymity_set.push(make_credential(&crs, seed).phi);
        }
        // Insert our credential at position 3
        anonymity_set.insert(3, ben.credential.phi);
        assert_eq!(anonymity_set.len(), 8);

        ben.register(0, anonymity_set).unwrap();
        assert_eq!(ben.index, Some(3));
        assert_eq!(ben.set_size(), Some(8));

        // Verify the commitment at our index matches
        let anon_set = ben.anonymity_set.as_ref().unwrap();
        assert_eq!(anon_set[ben.index.unwrap()], ben.credential.phi);
    }

    // ── Phase 3 tests ───────────────────────────────────────────────────────

    #[test]
    fn full_phase_3_service_registration_flow() {
        // Phase 0: CRS with 3 users (names)
        let crs = make_crs(3);

        // Phase 1: Create beneficiary
        let mut ben = make_beneficiary(&crs, 0xBB, "my-user");

        // Phase 2: Build anonymity set (N members for vtxo-tree)
        // Use unique secrets to avoid collisions with ben (seed 0xBB)
        let mut anonymity_set = Vec::new();
        for i in 0..7u16 {
            let sk = MasterSecret([(i & 0xFF) as u8; 32]);
            let r = ChildRandomness([((i + 1) & 0xFF) as u8; 32]);
            let k = BlindingKey([((i.wrapping_add(7)) & 0xFF) as u8; 32]);
            let name = FriendlyName::new(format!("filler-{i}"));
            let cred = MasterCredential::create(&crs, sk, r, k, name);
            anonymity_set.push(cred.phi);
        }
        // Insert our credential at position 5
        anonymity_set.insert(5, ben.credential.phi);
        assert_eq!(anonymity_set.len(), 8);

        ben.register(42, anonymity_set.clone()).unwrap();
        assert_eq!(ben.set_id, Some(42));

        // Phase 3: Register for service 2 (1-indexed)
        let service_reg = ben.create_payment_registration(&crs, 2).unwrap();

        // Check message fields
        assert_eq!(service_reg.service_index, 2);
        assert_eq!(service_reg.set_id, 42);
        assert!(service_reg.pseudonym[0] == 0x02 || service_reg.pseudonym[0] == 0x03);
        assert!(service_reg.public_nullifier[0] == 0x02 || service_reg.public_nullifier[0] == 0x03);

        // Verify the registration message
        assert!(verify_payment_identity_registration(&crs, &anonymity_set, &service_reg));

        // Cross-service replay: proof for service 2 must fail at service 1
        let mut replayed = PaymentIdentityRegistration {
            pseudonym: service_reg.pseudonym,
            public_nullifier: service_reg.public_nullifier,
            set_id: service_reg.set_id,
            service_index: 1, // wrong service
            friendly_name: service_reg.friendly_name.clone(),
            proof: service_reg.proof.clone(),
        };
        assert!(!verify_payment_identity_registration(&crs, &anonymity_set, &replayed));

        // Wrong set_id must also fail
        replayed.service_index = 2;
        replayed.set_id = 999;
        assert!(!verify_payment_identity_registration(&crs, &anonymity_set, &replayed));
    }
}
