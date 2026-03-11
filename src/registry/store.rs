use std::collections::HashSet;
use crate::core::{AnonymitySet, Commitment, Nullifier};

/// Default number of commitments per anonymity set.
pub const DEFAULT_SET_CAPACITY: usize = 1024;

/// In-memory registry state.
///
/// Holds two parallel structures:
/// - `sets`: the anonymity sets of commitments (the public anonymity set Λ).
/// - `nullifiers`: a flat index of every registered nullifier for O(1) Sybil
///   resistance checks.
pub struct RegistryStore {
    pub sets: Vec<AnonymitySet>,
    pub nullifiers: HashSet<Nullifier>,
    pub set_capacity: usize,
}

impl RegistryStore {
    pub fn new(set_capacity: usize) -> Self {
        let first_set = AnonymitySet::new(0, set_capacity);
        Self {
            sets: vec![first_set],
            nullifiers: HashSet::new(),
            set_capacity,
        }
    }

    /// Reconstruct from data loaded out of persistent storage.
    pub fn from_parts(
        sets: Vec<AnonymitySet>,
        nullifiers: HashSet<Nullifier>,
        set_capacity: usize,
    ) -> Self {
        Self { sets, nullifiers, set_capacity }
    }

    /// Returns `true` if this nullifier is already registered.
    pub fn has_nullifier(&self, nul: &Nullifier) -> bool {
        self.nullifiers.contains(nul)
    }

    /// Register a commitment + nullifier pair (single nullifier).
    ///
    /// Returns `RegisterResult` on success, or an error if the nullifier has
    /// already been used (Sybil resistance).
    pub fn register(
        &mut self,
        commitment: Commitment,
        nullifier: Nullifier,
    ) -> Result<RegisterResult, RegisterError> {
        if self.nullifiers.contains(&nullifier) {
            return Err(RegisterError::NullifierAlreadyUsed);
        }

        // Seal current set if full and open a new one.
        let new_set_opened = self.current_set().is_full();
        if new_set_opened {
            let next_id = self.sets.len() as u64;
            self.sets.push(AnonymitySet::new(next_id, self.set_capacity));
        }

        // Collect set_id and index before dropping the mutable borrow on `set`,
        // so we can then mutably borrow `self.nullifiers`.
        let set_id;
        let index;
        {
            let set = self.current_set_mut();
            set_id = set.id;
            index = set.commitments.len();
            set.push(commitment);
        }
        self.nullifiers.insert(nullifier);

        Ok(RegisterResult { set_id, index, new_set_opened })
    }

    /// Register a master identity commitment with ALL per-service-provider nullifiers.
    ///
    /// This is `addID(Φ)` from the ASC specification. The commitment is the
    /// multi-value Pedersen commitment (master identity), and the nullifiers
    /// are ALL L per-verifier nullifiers for Sybil resistance.
    ///
    /// ALL nullifiers are checked for duplicates. If any is already registered,
    /// the entire registration is rejected.
    pub fn register_identity(
        &mut self,
        commitment: Commitment,
        nullifiers: Vec<Nullifier>,
    ) -> Result<RegisterResult, RegisterError> {
        // Check ALL nullifiers for duplicates before making any changes.
        for nul in &nullifiers {
            if self.nullifiers.contains(nul) {
                return Err(RegisterError::NullifierAlreadyUsed);
            }
        }

        // Seal current set if full and open a new one.
        let new_set_opened = self.current_set().is_full();
        if new_set_opened {
            let next_id = self.sets.len() as u64;
            self.sets.push(AnonymitySet::new(next_id, self.set_capacity));
        }

        let set_id;
        let index;
        {
            let set = self.current_set_mut();
            set_id = set.id;
            index = set.commitments.len();
            set.push(commitment);
        }

        // Insert ALL nullifiers.
        for nul in nullifiers {
            self.nullifiers.insert(nul);
        }

        Ok(RegisterResult { set_id, index, new_set_opened })
    }

    pub fn get_set(&self, id: u64) -> Option<&AnonymitySet> {
        self.sets.get(id as usize)
    }

    /// Check if an anonymity set is full (sealed) and ready for use.
    ///
    /// Returns `None` if the set doesn't exist, `Some(true)` if sealed,
    /// `Some(false)` if still accepting registrations.
    pub fn is_set_sealed(&self, set_id: u64) -> Option<bool> {
        self.get_set(set_id).map(|s| s.is_full())
    }

    fn current_set(&self) -> &AnonymitySet {
        self.sets.last().expect("sets is never empty")
    }

    fn current_set_mut(&mut self) -> &mut AnonymitySet {
        self.sets.last_mut().expect("sets is never empty")
    }
}

impl Default for RegistryStore {
    fn default() -> Self {
        Self::new(DEFAULT_SET_CAPACITY)
    }
}

#[derive(Debug)]
pub struct RegisterResult {
    pub set_id: u64,
    pub index: usize,
    /// `true` if a brand-new anonymity set was opened for this registration.
    pub new_set_opened: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegisterError {
    NullifierAlreadyUsed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_commitment(n: u8) -> Commitment { Commitment([n; 33]) }
    fn make_nullifier(n: u8) -> Nullifier { Nullifier([n; 32]) }

    #[test]
    fn register_and_has() {
        let mut store = RegistryStore::new(4);
        let nul = make_nullifier(1);
        assert!(!store.has_nullifier(&nul));
        store.register(make_commitment(1), nul).unwrap();
        assert!(store.has_nullifier(&nul));
    }

    #[test]
    fn duplicate_nullifier_rejected() {
        let mut store = RegistryStore::new(4);
        let nul = make_nullifier(1);
        store.register(make_commitment(1), nul).unwrap();
        assert!(matches!(
            store.register(make_commitment(2), nul),
            Err(RegisterError::NullifierAlreadyUsed)
        ));
    }

    #[test]
    fn register_identity_with_multiple_nullifiers() {
        let mut store = RegistryStore::new(4);
        let nuls = vec![make_nullifier(1), make_nullifier(2), make_nullifier(3)];
        let result = store.register_identity(make_commitment(1), nuls).unwrap();
        assert_eq!(result.set_id, 0);
        assert_eq!(result.index, 0);
        // All 3 nullifiers should be registered
        assert!(store.has_nullifier(&make_nullifier(1)));
        assert!(store.has_nullifier(&make_nullifier(2)));
        assert!(store.has_nullifier(&make_nullifier(3)));
    }

    #[test]
    fn register_identity_rejects_any_duplicate_nullifier() {
        let mut store = RegistryStore::new(4);
        // Register first identity with nullifiers 1,2,3
        store
            .register_identity(make_commitment(1), vec![make_nullifier(1), make_nullifier(2), make_nullifier(3)])
            .unwrap();
        // Second identity shares nullifier 2 — should be rejected
        let result = store.register_identity(
            make_commitment(2),
            vec![make_nullifier(4), make_nullifier(2), make_nullifier(5)],
        );
        assert!(matches!(result, Err(RegisterError::NullifierAlreadyUsed)));
        // Nullifiers 4 and 5 should NOT have been inserted (atomic rejection)
        assert!(!store.has_nullifier(&make_nullifier(4)));
        assert!(!store.has_nullifier(&make_nullifier(5)));
    }

    #[test]
    fn set_rolls_over_when_full() {
        let mut store = RegistryStore::new(2);
        let r0 = store.register(make_commitment(1), make_nullifier(1)).unwrap();
        let r1 = store.register(make_commitment(2), make_nullifier(2)).unwrap();
        // Set 0 is now full (capacity 2); next registration opens set 1.
        let r2 = store.register(make_commitment(3), make_nullifier(3)).unwrap();
        assert_eq!(r0.set_id, 0);
        assert_eq!(r1.set_id, 0);
        assert_eq!(r2.set_id, 1);
        assert!(r2.new_set_opened);
        assert!(store.get_set(0).unwrap().is_full());
        assert_eq!(store.sets.len(), 2);
    }
}
