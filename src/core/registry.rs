//! Identity Registry (Phase 0 + Phase 2).
//!
//! The Registry manages the CRS (Common Reference String) and the anonymity
//! set of beneficiary commitments. It is the on-chain component that:
//!
//! - Phase 0: Sets up the CRS with registered merchants.
//! - Phase 2: Collects beneficiary commitments Φ into anonymity sets.
//!
//! ```text
//! Registry = (crs, set_id, Λ_{d̂})
//! ```

use crate::core::crs::Crs;
use crate::core::merchant::Merchant;
use crate::core::types::Commitment;

/// The Identity Registry.
///
/// Collects merchants and beneficiary commitments, then creates the CRS
/// when `setup()` is called. The lifecycle is:
///
/// 1. `Registry::new()` — empty registry.
/// 2. `add_merchant()` — accumulate merchants.
/// 3. `setup()` — create the CRS from collected merchants (Phase 0 complete).
/// 4. `add_beneficiary()` — collect Φ commitments (Phase 2).
#[derive(Debug, Clone)]
pub struct Registry {
    /// The Common Reference String (created by `setup()`).
    pub crs: Crs,
    /// Merchants collected before CRS setup.
    merchants: Vec<Merchant>,
    /// Anonymity set size N.
    set_size: usize,
    /// Current anonymity set `Λ_{d̂} = [Φ_1, ..., Φ_N]`.
    pub anonymity_set: Vec<Commitment>,
    /// Current set ID `d̂`.
    pub set_id: u64,
}

impl Registry {
    /// Create a new empty registry with the given anonymity set size.
    ///
    /// Use `add_merchant()` to register merchants, then call `setup()`
    /// to create the CRS.
    pub fn new(set_size: usize) -> Self {
        Registry {
            crs: Crs::setup(Vec::new(), set_size),
            merchants: Vec::new(),
            set_size,
            anonymity_set: Vec::new(),
            set_id: 0,
        }
    }

    /// Register a merchant with the registry.
    ///
    /// Must be called before `setup()`. Returns the 1-indexed position.
    pub fn add_merchant(&mut self, mut merchant: Merchant) -> usize {
        let id = self.merchants.len() + 1;
        merchant.merchant_id = id;
        self.merchants.push(merchant);
        id
    }

    /// Create the CRS from all collected merchants (Phase 0).
    ///
    /// Calls `Crs::setup(merchants, set_size)` to derive all generators.
    pub fn setup(&mut self) {
        self.crs = Crs::setup(self.merchants.clone(), self.set_size);
    }

    /// Add a beneficiary's commitment Φ to the current anonymity set.
    ///
    /// Returns the 0-based index of the beneficiary within the set.
    pub fn add_beneficiary(&mut self, phi: Commitment) -> usize {
        self.anonymity_set.push(phi);
        self.anonymity_set.len() - 1
    }

    /// Returns the current anonymity set.
    pub fn anonymity_set(&self) -> &[Commitment] {
        &self.anonymity_set
    }

    /// Returns the number of beneficiaries in the current anonymity set.
    pub fn beneficiary_count(&self) -> usize {
        self.anonymity_set.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::credential::MasterCredential;
    use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret};

    fn make_merchant(name: &str) -> Merchant {
        Merchant::new(name, &format!("https://{name}"))
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        MasterCredential::create(
            crs,
            MasterSecret([seed; 32]),
            ChildRandomness([seed.wrapping_add(1); 32]),
            BlindingKey([seed.wrapping_add(2); 32]),
            FriendlyName::new(format!("user-{seed:02x}")),
        )
    }

    fn make_registry(names: &[&str]) -> Registry {
        let mut registry = Registry::new(1024);
        for name in names {
            registry.add_merchant(make_merchant(name));
        }
        registry.setup();
        registry
    }

    #[test]
    fn setup_creates_crs_with_merchants() {
        let registry = make_registry(&["merchant_1", "merchant_2"]);
        assert_eq!(registry.crs.num_merchants(), 2);
        assert!(registry.anonymity_set.is_empty());
    }

    #[test]
    fn add_beneficiary_returns_index() {
        let mut registry = make_registry(&["m1"]);
        let cred0 = make_credential(&registry.crs, 0x01);
        let cred1 = make_credential(&registry.crs, 0x02);

        assert_eq!(registry.add_beneficiary(cred0.phi), 0);
        assert_eq!(registry.add_beneficiary(cred1.phi), 1);
        assert_eq!(registry.beneficiary_count(), 2);
    }

    #[test]
    fn anonymity_set_returns_all_commitments() {
        let mut registry = make_registry(&["m1"]);
        let cred = make_credential(&registry.crs, 0x42);
        registry.add_beneficiary(cred.phi);

        let set = registry.anonymity_set();
        assert_eq!(set.len(), 1);
        assert_eq!(set[0], cred.phi);
    }
}
