use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{self, Keypair, Message, PublicKey, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::{
    transaction, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
    XOnlyPublicKey,
};

/// Creates a P2TR (pay-to-taproot) output script for the given x-only public key.
///
/// This uses a key-spend-only output (no script tree).
pub fn p2tr_script(key: &XOnlyPublicKey) -> ScriptBuf {
    ScriptBuf::new_p2tr_tweaked(bitcoin::key::TweakedPublicKey::dangerous_assume_tweaked(*key))
}

/// Creates a leaf transaction with two user outputs.
///
/// - `parent_outpoint`: the outpoint this tx spends (from the parent node).
/// - `left_key`: x-only pubkey for the left user's output.
/// - `left_amount`: amount for the left user.
/// - `right_key`: x-only pubkey for the right user's output.
/// - `right_amount`: amount for the right user.
pub fn create_leaf_tx(
    parent_outpoint: OutPoint,
    left_key: &XOnlyPublicKey,
    left_amount: Amount,
    right_key: &XOnlyPublicKey,
    right_amount: Amount,
) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: parent_outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut {
                value: left_amount,
                script_pubkey: p2tr_script(left_key),
            },
            TxOut {
                value: right_amount,
                script_pubkey: p2tr_script(right_key),
            },
        ],
    }
}

/// Creates an internal (non-leaf) transaction with two outputs.
///
/// Each output is a P2TR output whose key is the aggregate of the
/// descendant users' keys for that subtree.
///
/// - `parent_outpoint`: the outpoint this tx spends (from the parent node).
/// - `left_key`: aggregate x-only pubkey for the left subtree.
/// - `left_amount`: total value flowing to the left subtree.
/// - `right_key`: aggregate x-only pubkey for the right subtree.
/// - `right_amount`: total value flowing to the right subtree.
pub fn create_internal_tx(
    parent_outpoint: OutPoint,
    left_key: &XOnlyPublicKey,
    left_amount: Amount,
    right_key: &XOnlyPublicKey,
    right_amount: Amount,
) -> Transaction {
    // Same structure as a leaf tx — the difference is semantic:
    // leaf outputs pay to individual users, internal outputs pay to
    // aggregate keys that child transactions can spend.
    create_leaf_tx(
        parent_outpoint,
        left_key,
        left_amount,
        right_key,
        right_amount,
    )
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

/// Simple key aggregation: XOR-based placeholder.
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
