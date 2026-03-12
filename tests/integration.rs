use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::{Amount, OutPoint, Txid};
use std::collections::HashSet;
use veiled::core::tx::{build_identity_tree, IdentityTXO};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_user(seed: u8, sats: u64) -> IdentityTXO {
    let secp = Secp256k1::new();
    let mut secret = [0u8; 32];
    secret[31] = seed.max(1);
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

fn funding_outpoint() -> OutPoint {
    // Simulate a real funding outpoint (non-null).
    OutPoint {
        txid: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .parse::<Txid>()
            .unwrap(),
        vout: 0,
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

/// The root transaction's input must reference the funding outpoint.
#[test]
fn root_spends_funding_outpoint() {
    let fp = funding_outpoint();
    let users = make_users(8, 1_000);
    let tree = build_identity_tree(&users, fp).unwrap();

    assert_eq!(tree.root_tx.input.len(), 1);
    assert_eq!(tree.root_tx.input[0].previous_output, fp);
}

/// Root tx has 1 input and 1 output; fan-out tx has 1 input and N outputs.
#[test]
fn tx_structure() {
    let users = make_users(16, 500);
    let tree = build_identity_tree(&users, funding_outpoint()).unwrap();

    assert_eq!(tree.root_tx.input.len(), 1);
    assert_eq!(tree.root_tx.output.len(), 1, "root has 1 aggregate output");

    assert_eq!(tree.fanout_tx.input.len(), 1);
    assert_eq!(tree.fanout_tx.output.len(), 16, "fan-out has N outputs");
}

/// The fan-out transaction's input must reference the root's output 0.
#[test]
fn fanout_spends_root_output() {
    let tree = build_identity_tree(&make_users(32, 1_000), funding_outpoint()).unwrap();

    let root_txid = tree.root_tx.compute_txid();
    assert_eq!(tree.fanout_tx.input[0].previous_output.txid, root_txid);
    assert_eq!(tree.fanout_tx.input[0].previous_output.vout, 0);
}

/// Root and fan-out transactions have different txids.
#[test]
fn txids_are_unique() {
    let tree = build_identity_tree(&make_users(64, 100), funding_outpoint()).unwrap();
    assert_ne!(
        tree.root_tx.compute_txid(),
        tree.fanout_tx.compute_txid(),
        "root and fan-out must have different txids"
    );
}

/// Value is conserved: sum of fan-out outputs equals root output value.
#[test]
fn value_conservation() {
    let sats_each = 2_500u64;
    let n = 128;
    let users = make_users(n, sats_each);
    let tree = build_identity_tree(&users, funding_outpoint()).unwrap();

    let total_expected = Amount::from_sat(sats_each * n as u64);

    // Total value via helper
    assert_eq!(tree.value(), total_expected);

    // Root output value
    assert_eq!(tree.root_tx.output[0].value, total_expected);

    // Sum of fan-out outputs
    let fanout_sum: Amount = tree.fanout_tx.output.iter().map(|o| o.value).sum();
    assert_eq!(fanout_sum, total_expected);
}

/// Every output must use a P2TR script.
#[test]
fn all_outputs_are_p2tr() {
    let tree = build_identity_tree(&make_users(8, 1_000), funding_outpoint()).unwrap();

    for out in &tree.root_tx.output {
        assert!(out.script_pubkey.is_p2tr(), "root output must be P2TR");
    }
    for out in &tree.fanout_tx.output {
        assert!(out.script_pubkey.is_p2tr(), "fan-out output must be P2TR");
    }
}

/// Fan-out outputs must correspond to the correct users in input order.
#[test]
fn fanout_outputs_match_user_order() {
    let users = make_users(8, 1_000);
    let tree = build_identity_tree(&users, funding_outpoint()).unwrap();

    assert_eq!(tree.fanout_tx.output.len(), users.len());

    for (i, (out, user)) in tree.fanout_tx.output.iter().zip(users.iter()).enumerate() {
        assert_eq!(out.value, user.amount, "user {i} amount mismatch");
        // Verify the script pays to the user's key
        let (xonly, _) = user.pubkey.x_only_public_key();
        let expected_script = veiled::core::tx::p2tr_script(&xonly);
        assert_eq!(out.script_pubkey, expected_script, "user {i} script mismatch");
    }
}

/// Every user's branch has length 2 (root + fan-out).
#[test]
fn branch_lengths() {
    for n in [2, 4, 8, 16, 32, 64] {
        let tree = build_identity_tree(&make_users(n, 100), funding_outpoint()).unwrap();

        for i in 0..n {
            let branch = tree.branch(i).expect("branch must exist");
            assert_eq!(
                branch.len(),
                2,
                "user {i} in {n}-user tree: branch should be [root, fanout]"
            );
        }

        // Out of range
        assert!(tree.branch(n).is_none());
    }
}

/// Two trees built from the same users and funding outpoint must be identical.
#[test]
fn deterministic_construction() {
    let users = make_users(16, 1_234);
    let fp = funding_outpoint();
    let tree1 = build_identity_tree(&users, fp).unwrap();
    let tree2 = build_identity_tree(&users, fp).unwrap();

    assert_eq!(tree1.root_tx.compute_txid(), tree2.root_tx.compute_txid());
    assert_eq!(tree1.fanout_tx.compute_txid(), tree2.fanout_tx.compute_txid());
}

/// Changing any single user's key produces a different tree.
#[test]
fn different_users_produce_different_tree() {
    let users_a = make_users(8, 1_000);
    let users_b = {
        let mut u = users_a.clone();
        u[3] = make_user(200, 1_000);
        u
    };

    let fp = funding_outpoint();
    let tree_a = build_identity_tree(&users_a, fp).unwrap();
    let tree_b = build_identity_tree(&users_b, fp).unwrap();

    assert_ne!(
        tree_a.root_tx.compute_txid(),
        tree_b.root_tx.compute_txid(),
        "different users must produce different root txid"
    );
}

/// 1024-user tree: full-scale structure verification.
#[test]
fn full_scale_1024_users() {
    let n = 1024;
    let sats_each = 500u64;
    let users = make_users(n, sats_each);
    let tree = build_identity_tree(&users, funding_outpoint()).unwrap();

    // Structure
    assert_eq!(tree.user_count(), 1024);
    assert_eq!(tree.tx_count(), 2);

    // Value
    assert_eq!(tree.value(), Amount::from_sat(sats_each * n as u64));

    // Every user has a branch of length 2
    let branch_first = tree.branch(0).unwrap();
    let branch_last = tree.branch(1023).unwrap();
    assert_eq!(branch_first.len(), 2);
    assert_eq!(branch_last.len(), 2);

    // All branches share the same root and fan-out tx
    assert_eq!(
        branch_first[0].compute_txid(),
        branch_last[0].compute_txid(),
        "all branches share the root"
    );
    assert_eq!(
        branch_first[1].compute_txid(),
        branch_last[1].compute_txid(),
        "all branches share the fan-out"
    );
}
