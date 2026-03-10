//! Bitcoin anchoring for sealed anonymity sets.
//!
//! When an anonymity set fills to N=1024 commitments, it can be anchored
//! on Bitcoin via a vtxo-tree. Each commitment (a 33-byte compressed
//! secp256k1 point) becomes a leaf in the tree — the on-chain output is
//! locked to the commitment point itself.
//!
//! This replaces the Ethereum smart contract `IdR` from the ASC paper:
//! - `addID(Φ)` → off-chain registration, then tree construction when set is sealed
//! - `getIDs(index)` → read from the sealed set
//! - `getIDSize()` → set size

use bitcoin::secp256k1::PublicKey;
use bitcoin::{Amount, OutPoint, Transaction};
use veiled_core::{AnonymitySet, Commitment};
use vtxo_tree::tree::build_tree;
use vtxo_tree::types::{TreeNode, User};

/// Configuration for anchoring anonymity sets on Bitcoin.
pub struct AnchorConfig {
    /// Amount per leaf slot (minimum output value, e.g., 546 sats dust limit).
    pub leaf_amount: Amount,
}

impl Default for AnchorConfig {
    fn default() -> Self {
        Self {
            leaf_amount: Amount::from_sat(546),
        }
    }
}

/// A sealed anonymity set that has been anchored on Bitcoin via a vtxo-tree.
#[derive(Debug)]
pub struct AnchoredSet {
    /// The anonymity set ID.
    pub set_id: u64,
    /// The root transaction of the vtxo-tree (to be broadcast on-chain).
    pub root_tx: Transaction,
    /// The full vtxo-tree (for branch extraction / unilateral exit).
    pub tree: TreeNode,
    /// The funding outpoint used.
    pub funding_outpoint: OutPoint,
}

/// Build a vtxo-tree from a sealed anonymity set of 1024 commitments.
///
/// Each commitment is a 33-byte compressed secp256k1 point, which IS a
/// valid public key. The commitment becomes the leaf's P2TR key directly.
/// The on-chain output is locked to the commitment point — spending requires
/// proving knowledge of the commitment opening via the ASC proof protocol.
///
/// This is the Bitcoin equivalent of the Ethereum `IdR` contract:
/// once the anonymity set is sealed (full at N=1024), the commitments
/// are anchored on-chain via a pre-signed transaction tree.
pub fn anchor_anonymity_set(
    set: &AnonymitySet,
    funding_outpoint: OutPoint,
    config: &AnchorConfig,
) -> Result<AnchoredSet, String> {
    if !set.is_full() {
        return Err(format!(
            "anonymity set must be full ({}) before anchoring, got {}",
            set.capacity,
            set.commitments.len()
        ));
    }

    // Convert each commitment to a vtxo-tree User.
    // The commitment IS a valid secp256k1 point (33-byte compressed),
    // so it can be used directly as the leaf's public key.
    let users: Vec<User> = set
        .commitments
        .iter()
        .map(|c| commitment_to_user(c, config))
        .collect::<Result<Vec<_>, _>>()?;

    let tree = build_tree(&users, funding_outpoint).map_err(|e| e.to_string())?;

    Ok(AnchoredSet {
        set_id: set.id,
        root_tx: tree.tx().clone(),
        tree,
        funding_outpoint,
    })
}

/// Derives a secp256k1 PublicKey from a commitment for use as a vtxo-tree leaf key.
///
/// Since Pedersen commitments on secp256k1 are valid curve points, the
/// 33-byte compressed commitment bytes are a valid compressed public key.
fn commitment_to_user(commitment: &Commitment, config: &AnchorConfig) -> Result<User, String> {
    let pk = PublicKey::from_slice(commitment.as_bytes())
        .map_err(|e| format!("invalid commitment point: {e}"))?;
    Ok(User {
        pubkey: pk,
        amount: config.leaf_amount,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use veiled_core::{AnonymitySet, BlindingKey, Commitment, MasterSecret, Name, Nullifier};

    /// Create a valid commitment (actual secp256k1 point) using veiled-core.
    fn make_valid_commitment(seed: u8) -> Commitment {
        let nullifier = Nullifier([seed; 32]);
        let blinding = BlindingKey([(seed.wrapping_add(1)); 32]);
        veiled_core::commit(&nullifier, &blinding)
    }

    fn make_full_set(capacity: usize) -> AnonymitySet {
        let mut set = AnonymitySet::new(0, capacity);
        for i in 0..capacity {
            set.push(make_valid_commitment(i as u8));
        }
        set
    }

    #[test]
    fn rejects_incomplete_set() {
        let set = AnonymitySet::new(0, 1024); // empty
        let result = anchor_anonymity_set(&set, OutPoint::null(), &AnchorConfig::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be full"));
    }

    #[test]
    fn commitment_to_user_valid_point() {
        let c = make_valid_commitment(42);
        let config = AnchorConfig::default();
        let user = commitment_to_user(&c, &config).unwrap();
        assert_eq!(user.amount, Amount::from_sat(546));
        // The public key should be 33 bytes compressed
        assert_eq!(user.pubkey.serialize().len(), 33);
    }

    #[test]
    fn anchor_small_set() {
        // Use a small power-of-2 set for fast testing.
        let set = make_full_set(4);
        let config = AnchorConfig {
            leaf_amount: Amount::from_sat(1_000),
        };
        let anchored =
            anchor_anonymity_set(&set, OutPoint::null(), &config).expect("anchor should succeed");
        assert_eq!(anchored.set_id, 0);
        assert_eq!(anchored.tree.user_count(), 4);
        assert_eq!(anchored.tree.tx_count(), 3); // 1 root + 2 leaves
    }

    #[test]
    fn anchor_with_crs_commitments() {
        // Full flow: CRS setup → derive nullifiers → commit → anchor
        use veiled_core::{Crs, ServiceProvider};
        use veiled_core::nullifier_v2::derive_all_nullifiers;

        let providers: Vec<ServiceProvider> = (0..2)
            .map(|i| ServiceProvider {
                username: Name::new(format!("svc-{i}")),
                credential_generator: [0x02; 33],
                origin: format!("https://svc-{i}.com"),
            })
            .collect();
        let crs = Crs::setup(providers);

        // Create 4 master identities (CRS multi-value commitments)
        let mut set = AnonymitySet::new(0, 4);
        for seed in 1..=4u8 {
            let secret = MasterSecret([seed; 32]);
            let blinding = BlindingKey([seed.wrapping_add(100); 32]);
            let nullifiers = derive_all_nullifiers(&secret, &crs.usernames());
            let phi = crs.commit_master_identity(&nullifiers, &blinding).unwrap();
            set.push(phi);
        }
        assert!(set.is_full());

        let config = AnchorConfig {
            leaf_amount: Amount::from_sat(1_000),
        };
        let anchored =
            anchor_anonymity_set(&set, OutPoint::null(), &config).expect("anchor should succeed");
        assert_eq!(anchored.tree.user_count(), 4);
    }
}
