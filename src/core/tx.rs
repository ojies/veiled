use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{self, Keypair, Message, PublicKey, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::{
    transaction, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
    XOnlyPublicKey,
};


/// A participant in the transaction tree.
#[derive(Debug, Clone)]
pub struct IdentityTXO {
    /// The user's public key (used for their output).
    pub pubkey: PublicKey,
    /// The amount allocated to this user.
    pub amount: Amount,
}

/// A flat off-chain transaction structure.
///
/// ```text
///   funding_utxo
///       │
///   ┌───▼───┐
///   │ root  │  ← 1 input (funding), 1 output (aggregate key)
///   └───┬───┘
///       │
///   ┌───▼───┐
///   │fan-out│  ← 1 input (root output), N outputs (one per user)
///   └───────┘
/// ```
///
/// The root transaction commits to the group on-chain.
/// The fan-out transaction distributes funds to each user.
#[derive(Debug, Clone)]
pub struct IdentityTree {
    /// The root transaction: 1 input (funding UTXO), 1 output (aggregate key).
    pub root_tx: Transaction,
    /// The fan-out transaction: 1 input (spends root output 0), N outputs (one per user).
    pub fanout_tx: Transaction,
    /// The users in output order.
    pub users: Vec<IdentityTXO>,
}


impl IdentityTree {
    /// Returns the root transaction.
    pub fn root(&self) -> &Transaction {
        &self.root_tx
    }

    /// Returns the fan-out transaction.
    pub fn fanout(&self) -> &Transaction {
        &self.fanout_tx
    }

    /// Returns the total user-amount value (sum of all user amounts).
    pub fn value(&self) -> Amount {
        self.users.iter().map(|u| u.amount).sum()
    }

    /// Returns the number of users.
    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// Returns the total number of transactions (always 2: root + fan-out).
    pub fn tx_count(&self) -> usize {
        2
    }

    /// Returns the branch (list of transactions) from root to a user's output.
    ///
    /// For the flat structure this is always `[root_tx, fanout_tx]` —
    /// the user's specific output is at `fanout_tx.output[user_index]`.
    pub fn branch(&self, user_index: usize) -> Option<Vec<&Transaction>> {
        if user_index < self.users.len() {
            Some(vec![&self.root_tx, &self.fanout_tx])
        } else {
            None
        }
    }
}


/// Builds a flat transaction structure from a list of users.
///
/// Creates two transactions:
/// 1. **Root tx**: spends the funding UTXO, produces a single output locked
///    to the aggregate key of all users.
/// 2. **Fan-out tx**: spends the root output, produces N outputs — one per user.
///
/// # Requirements
/// - `users.len()` must be >= 2.
/// - `funding_outpoint` is the on-chain UTXO that funds the root transaction.
pub fn build_identity_tree(users: &[IdentityTXO], funding_outpoint: OutPoint) -> Result<IdentityTree, &'static str> {
    build_tree_with_fee(users, funding_outpoint, Amount::ZERO)
}

/// Like `build_tree`, but the root output includes extra value so the
/// fan-out transaction can pay a mining fee when broadcast.
pub fn build_tree_with_fee(
    users: &[IdentityTXO],
    funding_outpoint: OutPoint,
    fee: Amount,
) -> Result<IdentityTree, &'static str> {
    if users.len() < 2 {
        return Err("need at least 2 users");
    }

    // Aggregate key of all users (placeholder — MuSig2 in production).
    let all_keys: Vec<_> = users.iter().map(|u| u.pubkey).collect();
    let agg_key = aggregate_keys(&all_keys);

    // Total value flowing through the root output.
    let user_total: Amount = users.iter().map(|u| u.amount).sum();
    let root_output_value = user_total + fee;

    // 1. Root transaction: funding_outpoint → single aggregate output.
    let root_tx = create_root_tx(funding_outpoint, &agg_key, root_output_value);

    // 2. Fan-out transaction: spends root output 0 → N user outputs.
    let root_outpoint = OutPoint {
        txid: root_tx.compute_txid(),
        vout: 0,
    };
    let fanout_tx: bitcoin::Transaction = create_fanout_tx(root_outpoint, users);

    Ok(IdentityTree {
        root_tx,
        fanout_tx,
        users: users.to_vec(),
    })
}


/// Creates a P2TR (pay-to-taproot) output script for the given x-only public key.
///
/// This uses a key-spend-only output (no script tree).
pub fn p2tr_script(key: &XOnlyPublicKey) -> ScriptBuf {
    ScriptBuf::new_p2tr_tweaked(bitcoin::key::TweakedPublicKey::dangerous_assume_tweaked(*key))
}

/// Creates the root transaction: 1 input (funding UTXO), 1 output (aggregate key).
pub fn create_root_tx(
    funding_outpoint: OutPoint,
    aggregate_key: &XOnlyPublicKey,
    total_amount: Amount,
) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: funding_outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: total_amount,
            script_pubkey: p2tr_script(aggregate_key),
        }],
    }
}

/// Creates the fan-out transaction: 1 input (root output), N outputs (one per user).
pub fn create_fanout_tx(root_outpoint: OutPoint, users: &[IdentityTXO]) -> Transaction {
    let outputs: Vec<TxOut> = users
        .iter()
        .map(|u| {
            let (xonly, _) = u.pubkey.x_only_public_key();
            TxOut {
                value: u.amount,
                script_pubkey: p2tr_script(&xonly),
            }
        })
        .collect();

    Transaction {
        version: transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: root_outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: outputs,
    }
}

/// Computes the outpoint for a given output index of a transaction.
pub fn outpoint(tx: &Transaction, vout: u32) -> OutPoint {
    OutPoint {
        txid: tx.compute_txid(),
        vout,
    }
}

/// Returns the aggregate secret key for a set of public keys.
///
/// This mirrors `aggregate_keys` but returns the `SecretKey` so the
/// aggregate output can be spent.  Only works because our placeholder
/// aggregation derives the key deterministically from the sorted pubkeys.
pub fn aggregate_secret_key(keys: &[PublicKey]) -> SecretKey {
    use bitcoin::hashes::{sha256, Hash, HashEngine};

    let mut engine = sha256::Hash::engine();
    let mut sorted: Vec<&PublicKey> = keys.iter().collect();
    sorted.sort_by_key(|k| k.serialize());
    for k in &sorted {
        engine.input(&k.serialize());
    }
    let hash = sha256::Hash::from_engine(engine);
    SecretKey::from_slice(hash.as_ref()).expect("SHA256 output is a valid secret key")
}

/// Signs a transaction input (index 0) using BIP341 key-spend (Schnorr).
///
/// - `tx`: the transaction to sign (modified in place — witness is set).
/// - `secret_key`: the secret key controlling the spent output.
/// - `prev_output`: the `TxOut` being spent (needed for sighash).
pub fn sign_tx(tx: &mut Transaction, secret_key: &SecretKey, prev_output: &TxOut) {
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, secret_key);

    let mut sighash_cache = SighashCache::new(&*tx);
    let sighash = sighash_cache
        .taproot_key_spend_signature_hash(
            0,
            &Prevouts::All(&[prev_output]),
            TapSighashType::Default,
        )
        .expect("sighash computation failed");

    let msg = Message::from_digest(*sighash.as_byte_array());
    let sig = secp.sign_schnorr(&msg, &keypair);

    // BIP341 default sighash: 64-byte signature, no sighash byte appended.
    let mut witness = Witness::new();
    witness.push(sig.as_ref());
    tx.input[0].witness = witness;
}

/// Simple key aggregation: hash-based placeholder.
///
/// In production this would use MuSig2 key aggregation (BIP327).
/// For now we use a deterministic combination: sort the keys
/// lexicographically and hash them to derive an aggregate x-only key.
pub fn aggregate_keys(keys: &[PublicKey]) -> XOnlyPublicKey {
    use bitcoin::hashes::{sha256, Hash, HashEngine};

    let mut engine = sha256::Hash::engine();
    let mut sorted: Vec<&PublicKey> = keys.iter().collect();
    sorted.sort_by_key(|k| k.serialize());
    for k in &sorted {
        engine.input(&k.serialize());
    }
    let hash = sha256::Hash::from_engine(engine);

    // Use the hash as a secret key to derive a valid public key.
    // This is NOT cryptographically secure for MuSig2 — it's a
    // placeholder that produces a valid secp256k1 point.
    let secp = Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(hash.as_ref())
        .expect("SHA256 output is a valid secret key");
    let pk = sk.public_key(&secp);
    let (xonly, _parity) = pk.x_only_public_key();
    xonly
}


#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
    use bitcoin::Amount;

    fn make_user(seed: u8, sats: u64) -> IdentityTXO {
        let secp = Secp256k1::new();
        let mut secret = [0u8; 32];
        secret[31] = seed;
        if seed == 0 {
            secret[31] = 1;
        }
        let sk = SecretKey::from_slice(&secret).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);
        IdentityTXO {
            pubkey: pk,
            amount: Amount::from_sat(sats),
        }
    }

    fn make_users(count: usize, sats_each: u64) -> Vec<IdentityTXO> {
        (1..=count)
            .map(|i| make_user(i as u8, sats_each))
            .collect()
    }

    #[test]
    fn tree_2_users() {
        let users = make_users(2, 10_000);
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(tree.user_count(), 2);
        assert_eq!(tree.tx_count(), 2);
        assert_eq!(tree.value(), Amount::from_sat(20_000));

        // Fan-out has 2 outputs
        assert_eq!(tree.fanout_tx.output.len(), 2);
        assert_eq!(tree.fanout_tx.output[0].value, Amount::from_sat(10_000));
        assert_eq!(tree.fanout_tx.output[1].value, Amount::from_sat(10_000));
    }

    #[test]
    fn tree_4_users() {
        let users = make_users(4, 5_000);
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(tree.user_count(), 4);
        assert_eq!(tree.tx_count(), 2);
        assert_eq!(tree.value(), Amount::from_sat(20_000));

        // Root has 1 output (aggregate key)
        assert_eq!(tree.root_tx.output.len(), 1);
        assert_eq!(tree.root_tx.output[0].value, Amount::from_sat(20_000));

        // Fan-out has 4 outputs
        assert_eq!(tree.fanout_tx.output.len(), 4);

        // Fan-out spends root output 0
        let root_txid = tree.root_tx.compute_txid();
        assert_eq!(tree.fanout_tx.input[0].previous_output.txid, root_txid);
        assert_eq!(tree.fanout_tx.input[0].previous_output.vout, 0);
    }

    #[test]
    fn tree_1024_users() {
        let users = make_users(1024, 1_000);
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(tree.user_count(), 1024);
        assert_eq!(tree.tx_count(), 2);
        assert_eq!(tree.value(), Amount::from_sat(1_024_000));
    }

    #[test]
    fn branch_length() {
        let users = make_users(8, 1_000);
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        // Every branch is [root, fanout] = 2 transactions
        let branch = tree.branch(0).unwrap();
        assert_eq!(branch.len(), 2);

        let branch = tree.branch(7).unwrap();
        assert_eq!(branch.len(), 2);

        // Out-of-range returns None
        assert!(tree.branch(8).is_none());
    }

    #[test]
    fn branch_outpoints_chain_correctly() {
        let users = make_users(4, 5_000);
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        let branch = tree.branch(0).unwrap();
        assert_eq!(branch.len(), 2);

        // Fan-out's input spends root's output 0
        let root_txid = branch[0].compute_txid();
        assert_eq!(branch[1].input[0].previous_output.txid, root_txid);
        assert_eq!(branch[1].input[0].previous_output.vout, 0);
    }

    #[test]
    fn rejects_single_user() {
        let users = make_users(1, 1_000);
        assert!(build_identity_tree(&users, OutPoint::null()).is_err());
    }

    #[test]
    fn different_amounts_per_user() {
        let users = vec![make_user(1, 3_000), make_user(2, 7_000)];
        let tree = build_identity_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(tree.fanout_tx.output[0].value, Amount::from_sat(3_000));
        assert_eq!(tree.fanout_tx.output[1].value, Amount::from_sat(7_000));
        assert_eq!(tree.value(), Amount::from_sat(10_000));
    }
}
