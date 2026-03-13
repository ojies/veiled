//! Merchant (service provider) definition.
//!
//! Each merchant has a unique name, a deterministic credential generator
//! derived via HashToCurve, and an origin URL. After CRS setup, a merchant
//! can receive and verify payment identity registrations from beneficiaries.

use std::collections::HashMap;

use k256::{
    elliptic_curve::{
        group::GroupEncoding,
        hash2curve::{ExpandMsgXmd, GroupDigest},
    },
    Secp256k1,
};
use sha2::Sha256;

use crate::core::crs::Crs;
use crate::core::payment_identity::{
    PaymentIdentityRegistration, verify_payment_identity_registration,
};
use crate::core::types::{Commitment, Name};

/// Domain separation tag for CRS generator derivation.
const CRS_DST: &[u8] = b"CRS-ASC-v1";

/// A user registered in the CRS.
///
/// Each user has a unique name `v_l` (every user is also a service
/// provider), a credential generator `G_auth_l`, and an origin URL.
///
/// After receiving payment identity registrations, verified identities
/// are stored in `registered_identities` keyed by pseudonym.
#[derive(Debug, Clone)]
pub struct Merchant {
    /// Unique name v_l — every user is also a service provider.
    /// Used as the HKDF salt for nullifier derivation.
    pub name: Name,
    /// Credential generator G_auth_l (compressed secp256k1 point).
    /// Derived deterministically: `G_auth_l = HashToCurve("CRS-ASC-credential-generator-{name}")`.
    pub credential_generator: [u8; 33],
    /// Origin URL for the service provider.
    pub origin: String,
    /// 1-indexed position in the CRS (set by `Registry::add_merchant`).
    pub merchant_id: usize,
    /// Verified payment identity registrations, keyed by beneficiary pseudonym.
    pub registered_identities: HashMap<[u8; 33], PaymentIdentityRegistration>,
}

impl Merchant {
    /// Create a new merchant with a deterministic credential generator.
    ///
    /// ```text
    /// G_auth_l = HashToCurve("CRS-ASC-credential-generator-{name}", DST="CRS-ASC-v1")
    /// ```
    ///
    /// The generator is a NUMS (Nothing Up My Sleeve) point — nobody knows
    /// its discrete log relative to any other generator.
    pub fn new(name: &str, origin: &str) -> Self {
        let tag = format!("CRS-ASC-credential-generator-{name}");
        let point = Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[tag.as_bytes()],
            &[CRS_DST],
        )
        .expect("hash_to_curve never fails for secp256k1");
        let credential_generator: [u8; 33] = point.to_affine().to_bytes().into();
        Merchant {
            name: Name::new(name),
            credential_generator,
            origin: origin.to_string(),
            merchant_id: 0,
            registered_identities: HashMap::new(),
        }
    }

    /// Receive and verify a payment identity registration from a beneficiary.
    ///
    /// Calls `verify_payment_identity_registration` to validate the proof,
    /// then stores the registration keyed by the beneficiary's pseudonym.
    ///
    /// Returns `Ok(pseudonym)` on success, or an error if verification fails
    /// or the pseudonym is already registered.
    pub fn receive_payment_registration(
        &mut self,
        crs: &Crs,
        anonymity_set: &[Commitment],
        registration: &PaymentIdentityRegistration,
    ) -> Result<[u8; 33], &'static str> {
        if self.registered_identities.contains_key(&registration.pseudonym) {
            return Err("pseudonym already registered");
        }

        let valid = verify_payment_identity_registration(crs, anonymity_set, registration);
        if !valid {
            return Err("payment identity registration proof is invalid");
        }

        let pseudonym = registration.pseudonym;
        self.registered_identities
            .insert(pseudonym, registration.clone());
        Ok(pseudonym)
    }
}
