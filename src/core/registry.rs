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

use bitcoin::hashes::{Hash, sha256};
use bitcoin::secp256k1::{Keypair, PublicKey, Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::{
    transaction, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};

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
///
/// A Taproot commitment transaction anchoring the anonymity set on-chain.
#[derive(Debug, Clone)]
pub struct TaprootCommitment {
    /// The unsigned commitment transaction.
    pub tx: Transaction,
    /// Merkle root of the anonymity set (SHA256 binary tree of commitment hashes).
    pub merkle_root: [u8; 32],
    /// The Registry's internal key (x-only).
    pub internal_key: XOnlyPublicKey,
    /// The tweaked output key (internal_key + H(internal_key || merkle_root)).
    pub output_key: XOnlyPublicKey,
}

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
    /// Current set ID `d̂` — Merkle root of the commitment transaction.
    pub set_id: [u8; 32],
    /// Operator's secp256k1 keypair (internal key for Taproot).
    keypair: Keypair,
    /// Required payment amount per beneficiary (satoshis).
    amount: u64,
    /// UTXO each beneficiary paid when joining the anonymity set.
    beneficiary_outputs: Vec<OutPoint>,
}

impl Registry {
    /// Create a new empty registry with the given anonymity set size and
    /// per-beneficiary payment amount (in satoshis).
    ///
    /// Generates a fresh operator keypair. Use `add_merchant()` to register
    /// merchants, then call `setup()` to create the CRS.
    pub fn new(set_size: usize, amount: u64) -> Self {
        use rand_core::{OsRng, RngCore};

        let mut sk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut sk_bytes);
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&sk_bytes)
            .expect("32 random bytes should be a valid secret key");
        let keypair = Keypair::from_secret_key(&secp, &secret_key);

        Registry {
            crs: Crs::setup(Vec::new(), set_size),
            merchants: Vec::new(),
            set_size,
            anonymity_set: Vec::new(),
            set_id: [0u8; 32],
            keypair,
            amount,
            beneficiary_outputs: Vec::new(),
        }
    }

    /// Returns the operator's public key.
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_keypair(&self.keypair)
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
    pub fn setup(&mut self) -> Self {
        self.crs = Crs::setup(self.merchants.clone(), self.set_size);
        return self.clone()
    }

    /// Add a beneficiary's commitment Φ to the current anonymity set.
    ///
    /// `output` is the UTXO the beneficiary paid when joining.
    /// Returns the 0-based index of the beneficiary within the set.
    pub fn add_beneficiary(&mut self, phi: Commitment, output: OutPoint) -> usize {
        self.anonymity_set.push(phi);
        self.beneficiary_outputs.push(output);
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

    /// Create a Taproot commitment transaction anchoring the anonymity set.
    ///
    /// The transaction has a single input (the operator's `funding_outpoint`)
    /// and a single P2TR output whose internal key is the operator's public key,
    /// tweaked with the Merkle root of the anonymity set.
    ///
    /// Each Merkle leaf is `SHA256(Φ_i)` where `Φ_i` is the 33-byte commitment.
    pub fn create_anonymity_set(
        &mut self,
        funding_outpoint: OutPoint,
    ) -> Result<TaprootCommitment, &'static str> {
        if self.anonymity_set.is_empty() {
            return Err("anonymity set is empty");
        }

        // Step 1: Compute leaf hashes — SHA256 of each commitment.
        let mut leaves: Vec<[u8; 32]> = self
            .anonymity_set
            .iter()
            .map(|phi| sha256::Hash::hash(&phi.0).to_byte_array())
            .collect();

        // Pad to next power of 2 by duplicating the last leaf.
        let next_pow2 = leaves.len().next_power_of_two();
        while leaves.len() < next_pow2 {
            leaves.push(*leaves.last().unwrap());
        }

        // Step 2: Build binary Merkle tree bottom-up.
        let mut level = leaves;
        while level.len() > 1 {
            let mut next_level = Vec::with_capacity(level.len() / 2);
            for pair in level.chunks(2) {
                let mut preimage = [0u8; 64];
                preimage[..32].copy_from_slice(&pair[0]);
                preimage[32..].copy_from_slice(&pair[1]);
                next_level.push(sha256::Hash::hash(&preimage).to_byte_array());
            }
            level = next_level;
        }
        let merkle_root = level[0];
        self.set_id = merkle_root;

        // Step 3: Compute tweaked output key.
        //   output_key = internal_key + H(internal_key || merkle_root)
        let secp = Secp256k1::new();
        let (internal_key, _parity) = self.keypair.x_only_public_key();

        let mut tweak_preimage = [0u8; 64];
        tweak_preimage[..32].copy_from_slice(&internal_key.serialize());
        tweak_preimage[32..].copy_from_slice(&merkle_root);
        let tweak_hash = sha256::Hash::hash(&tweak_preimage).to_byte_array();

        let tweak = bitcoin::secp256k1::Scalar::from_be_bytes(tweak_hash)
            .map_err(|_| "tweak scalar overflow")?;
        let (output_key, _parity) = internal_key
            .add_tweak(&secp, &tweak)
            .map_err(|_| "tweak addition failed")?;

        // Step 4: Build the commitment transaction.
        let total_value = Amount::from_sat(self.amount * self.anonymity_set.len() as u64);
        let output_script = ScriptBuf::new_p2tr_tweaked(
            bitcoin::key::TweakedPublicKey::dangerous_assume_tweaked(output_key),
        );

        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: funding_outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            }],
            output: vec![TxOut {
                value: total_value,
                script_pubkey: output_script,
            }],
        };

        Ok(TaprootCommitment {
            tx,
            merkle_root,
            internal_key,
            output_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::credential::MasterCredential;
    use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret};
    use bitcoin::Txid;

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

    fn null_outpoint() -> OutPoint {
        OutPoint {
            txid: Txid::all_zeros(),
            vout: 0,
        }
    }

    fn make_registry(names: &[&str]) -> Registry {
        let mut registry = Registry::new(1024, 10_000);
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

        assert_eq!(registry.add_beneficiary(cred0.phi, null_outpoint()), 0);
        assert_eq!(registry.add_beneficiary(cred1.phi, null_outpoint()), 1);
        assert_eq!(registry.beneficiary_count(), 2);
    }

    #[test]
    fn anonymity_set_returns_all_commitments() {
        let mut registry = make_registry(&["m1"]);
        let cred = make_credential(&registry.crs, 0x42);
        registry.add_beneficiary(cred.phi, null_outpoint());

        let set = registry.anonymity_set();
        assert_eq!(set.len(), 1);
        assert_eq!(set[0], cred.phi);
    }

    #[test]
    fn public_key_is_valid() {
        let registry = make_registry(&["m1"]);
        let pk = registry.public_key();
        // Valid compressed public key starts with 0x02 or 0x03.
        let ser = pk.serialize();
        assert!(ser[0] == 0x02 || ser[0] == 0x03);
    }

    #[test]
    fn create_anonymity_set_empty_returns_error() {
        let mut registry = make_registry(&["m1"]);
        let err = registry.create_anonymity_set(null_outpoint()).unwrap_err();
        assert_eq!(err, "anonymity set is empty");
    }

    #[test]
    fn create_anonymity_set_builds_valid_taproot_tx() {
        let mut registry = make_registry(&["m1"]);
        for seed in 0x01..=0x08u8 {
            let cred = make_credential(&registry.crs, seed);
            registry.add_beneficiary(cred.phi, null_outpoint());
        }

        let commitment = registry
            .create_anonymity_set(null_outpoint())
            .expect("should build commitment tx");

        // 1 input, 1 output.
        assert_eq!(commitment.tx.input.len(), 1);
        assert_eq!(commitment.tx.output.len(), 1);

        // Output value = amount * 8 beneficiaries.
        let expected_value = Amount::from_sat(10_000 * 8);
        assert_eq!(commitment.tx.output[0].value, expected_value);

        // Output script is P2TR (OP_1 <32-byte key>).
        let script = &commitment.tx.output[0].script_pubkey;
        assert!(script.is_p2tr(), "output must be P2TR");

        // Merkle root is non-zero.
        assert_ne!(commitment.merkle_root, [0u8; 32]);
    }
}
