//! Beneficiary — a member of the anonymity set.
//!
//! Created during Phase 1 with a friendly name and random secrets.
//! After Phase 2 registration, the anonymity set and set ID are attached
//! via `register`, enabling Phase 3 proof generation.

use std::collections::HashMap;

use crate::core::credential::MasterCredential;
use crate::core::crs::Crs;
use crate::core::payment_identity::PaymentIdentityRegistration;
use crate::core::types::{BlindingKey, ChildRandomness, Commitment, FriendlyName, MasterSecret};

/// A beneficiary in the ASC protocol.
///
/// Created during Phase 1 with a friendly name and random secrets.
/// After Phase 2 registration, the anonymity set and set ID are attached
/// via `register`, enabling Phase 3 proof generation.
///
/// ```text
/// (Φ_j, sk, r, k, d̂, Λ_{d̂})
/// ```
#[derive(Debug, Clone)]
pub struct Beneficiary {
    /// The master credential `(Φ, sk, r, k)`.
    pub credential: MasterCredential,
    /// `d̂` — Merkle root of the commitment transaction (set after Phase 2).
    pub set_id: Option<[u8; 32]>,
    /// `j` — the user's index within `Λ_{d̂}` (0-based, set after Phase 2).
    pub index: Option<usize>,
    /// `Λ_{d̂}` — the complete frozen anonymity set (set after Phase 2).
    pub anonymity_set: Option<Vec<Commitment>>,
    /// Payment identity registrations keyed by merchant_id (1-indexed).
    pub registrations: HashMap<usize, PaymentIdentityRegistration>,
}

impl Beneficiary {
    /// Create a new beneficiary (Phase 1).
    ///
    /// Generates random secrets `(sk, r, k)` and creates the master credential.
    /// The anonymity set and set ID are not yet known — call `register` after
    /// Phase 2 completes.
    pub fn new(crs: &Crs, friendly_name: &str) -> Self {
        use rand_core::{OsRng, RngCore};

        let mut sk_bytes = [0u8; 32];
        let mut r_bytes = [0u8; 32];
        let mut k_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut sk_bytes);
        OsRng.fill_bytes(&mut r_bytes);
        OsRng.fill_bytes(&mut k_bytes);

        let credential = MasterCredential::create(
            crs,
            MasterSecret(sk_bytes),
            ChildRandomness(r_bytes),
            BlindingKey(k_bytes),
            FriendlyName::new(friendly_name),
        );
        Self {
            credential,
            set_id: None,
            index: None,
            anonymity_set: None,
            registrations: HashMap::new(),
        }
    }

    /// Attach the anonymity set after Phase 2 registration completes.
    ///
    /// Finds the user's index `j` by searching for their `Φ` in the set.
    /// Returns an error if `Φ` is not found in the anonymity set.
    pub fn register(
        &mut self,
        set_id: [u8; 32],
        anonymity_set: Vec<Commitment>,
    ) -> Result<(), &'static str> {
        let index = anonymity_set
            .iter()
            .position(|c| *c == self.credential.phi)
            .ok_or("master identity not found in anonymity set")?;
        self.set_id = Some(set_id);
        self.index = Some(index);
        self.anonymity_set = Some(anonymity_set);
        Ok(())
    }

    /// Returns the number of identities in the anonymity set.
    pub fn set_size(&self) -> Option<usize> {
        self.anonymity_set.as_ref().map(|s| s.len())
    }

    /// Register for a specific merchant (Phase 3).
    ///
    /// Generates the full service registration message:
    /// ```text
    /// (ϕ, nul_l, π, d̂)
    /// ```
    ///
    /// - `merchant_id`: 1-indexed merchant in the CRS.
    ///
    /// The registration is stored internally keyed by `merchant_id`.
    /// Returns a clone of the stored `PaymentIdentityRegistration`.
    pub fn create_payment_registration(
        &mut self,
        crs: &Crs,
        merchant_id: usize,
    ) -> Result<PaymentIdentityRegistration, &'static str> {
        use crate::core::request::derive_payment_request_pseudonym;
        use crate::core::payment_identity::prove_payment_identity_registration;

        let anonymity_set = self.anonymity_set.as_ref().ok_or("not yet registered (no anonymity set)")?;
        let index = self.index.ok_or("not yet registered (no index)")?;
        let set_id = self.set_id.ok_or("not yet registered (no set_id)")?;

        if merchant_id < 1 || merchant_id > crs.num_merchants() {
            return Err("merchant_id out of range");
        }

        let name = &crs.merchants[merchant_id - 1].name;
        let pseudonym = derive_payment_request_pseudonym(&self.credential.r, name, &crs.g);
        let pub_nul = self.credential.public_nullifier(crs, merchant_id);
        let all_nullifiers = self.credential.all_nullifier_scalars(crs);

        let name_scalar = self.credential.friendly_name.to_scalar_bytes();
        let proof = prove_payment_identity_registration(
            crs,
            anonymity_set,
            index,
            merchant_id,
            &set_id,
            &self.credential.k.0,
            &all_nullifiers,
            &pseudonym,
            &pub_nul,
            &name_scalar,
        )?;

        let registration = PaymentIdentityRegistration {
            pseudonym,
            public_nullifier: pub_nul,
            set_id,
            service_index: merchant_id,
            friendly_name: self.credential.friendly_name.as_str().to_string(),
            proof,
        };

        self.registrations.insert(merchant_id, registration.clone());
        Ok(registration)
    }
}
