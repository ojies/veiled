use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::{Amount, OutPoint, Txid};
use std::collections::HashSet;
use vtxo_tree::tree::build_tree;
use vtxo_tree::types::{TreeNode, User};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_user(seed: u8, sats: u64) -> User {
    let secp = Secp256k1::new();
    let mut secret = [0u8; 32];
    secret[31] = seed.max(1);
    let sk = SecretKey::from_slice(&secret).unwrap();
    let pk = PublicKey::from_secret_key(&secp, &sk);
    User {
        pubkey: pk,
        amount: Amount::from_sat(sats),
    }
}

fn make_users(count: usize, sats_each: u64) -> Vec<User> {
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

/// Recursively collect every transaction in the tree (DFS order).
fn collect_all_txs(node: &TreeNode) -> Vec<&bitcoin::Transaction> {
    let mut txs = vec![node.tx()];
    if let TreeNode::Internal { left, right, .. } = node {
        txs.extend(collect_all_txs(left));
        txs.extend(collect_all_txs(right));
    }
    txs
}

/// Recursively collect all leaf user outputs (left, right pairs).
fn collect_leaf_outputs(node: &TreeNode) -> Vec<(&User, &User)> {
    match node {
        TreeNode::Leaf {
            left_user,
            right_user,
            ..
        } => vec![(left_user, right_user)],
        TreeNode::Internal { left, right, .. } => {
            let mut out = collect_leaf_outputs(left);
            out.extend(collect_leaf_outputs(right));
            out
        }
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

/// The root transaction's input must reference the funding outpoint.
#[test]
fn root_spends_funding_outpoint() {
    let fp = funding_outpoint();
    let users = make_users(8, 1_000);
    let root = build_tree(&users, fp).unwrap();

    assert_eq!(root.tx().input.len(), 1);
    assert_eq!(root.tx().input[0].previous_output, fp);
}

/// Every transaction in the tree must have exactly 1 input and 2 outputs.
#[test]
fn all_txs_have_1_input_2_outputs() {
    let root = build_tree(&make_users(16, 500), funding_outpoint()).unwrap();
    for tx in collect_all_txs(&root) {
        assert_eq!(tx.input.len(), 1, "every tree tx has exactly 1 input");
        assert_eq!(tx.output.len(), 2, "every tree tx has exactly 2 outputs");
    }
}

/// Every child transaction's input must reference its parent's output.
/// This verifies the full outpoint chain from root to every leaf.
#[test]
fn full_outpoint_chain_integrity() {
    let root = build_tree(&make_users(32, 1_000), funding_outpoint()).unwrap();
    verify_outpoint_chain(&root);
}

fn verify_outpoint_chain(node: &TreeNode) {
    if let TreeNode::Internal {
        tx, left, right, ..
    } = node
    {
        let parent_txid = tx.compute_txid();

        // Left child spends output 0
        assert_eq!(left.tx().input[0].previous_output.txid, parent_txid);
        assert_eq!(left.tx().input[0].previous_output.vout, 0);

        // Right child spends output 1
        assert_eq!(right.tx().input[0].previous_output.txid, parent_txid);
        assert_eq!(right.tx().input[0].previous_output.vout, 1);

        verify_outpoint_chain(left);
        verify_outpoint_chain(right);
    }
}

/// Every transaction in the tree must have a unique txid.
#[test]
fn all_txids_are_unique() {
    let root = build_tree(&make_users(64, 100), funding_outpoint()).unwrap();
    let txs = collect_all_txs(&root);
    let txids: HashSet<_> = txs.iter().map(|tx| tx.compute_txid()).collect();
    assert_eq!(txids.len(), txs.len(), "every txid must be unique");
}

/// Value is conserved: sum of leaf outputs equals sum of root outputs.
#[test]
fn value_conservation() {
    let sats_each = 2_500u64;
    let n = 128;
    let users = make_users(n, sats_each);
    let root = build_tree(&users, funding_outpoint()).unwrap();

    let total_expected = Amount::from_sat(sats_each * n as u64);

    // Root-level value
    assert_eq!(root.value(), total_expected);

    // Sum of root tx outputs
    let root_output_sum: Amount = root.tx().output.iter().map(|o| o.value).sum();
    assert_eq!(root_output_sum, total_expected);

    // Sum of all leaf user outputs
    let leaf_pairs = collect_leaf_outputs(&root);
    let leaf_sum: Amount = leaf_pairs
        .iter()
        .map(|(l, r)| l.amount + r.amount)
        .sum();
    assert_eq!(leaf_sum, total_expected);
}

/// At every internal node, the sum of child output values must equal
/// the node's output values.
#[test]
fn value_conservation_at_every_level() {
    let root = build_tree(&make_users(16, 3_000), funding_outpoint()).unwrap();
    verify_value_conservation(&root);
}

fn verify_value_conservation(node: &TreeNode) {
    if let TreeNode::Internal {
        tx, left, right, ..
    } = node
    {
        // Parent's output 0 value == left subtree total
        assert_eq!(tx.output[0].value, left.value());
        // Parent's output 1 value == right subtree total
        assert_eq!(tx.output[1].value, right.value());

        verify_value_conservation(left);
        verify_value_conservation(right);
    }
}

/// Every leaf output must use a P2TR script (starts with OP_1 <32 bytes>).
#[test]
fn leaf_outputs_are_p2tr() {
    let root = build_tree(&make_users(8, 1_000), funding_outpoint()).unwrap();
    let txs = collect_all_txs(&root);
    for tx in txs {
        for out in &tx.output {
            assert!(
                out.script_pubkey.is_p2tr(),
                "output script must be P2TR, got: {:?}",
                out.script_pubkey
            );
        }
    }
}

/// Leaf outputs must correspond to the correct users in input order.
#[test]
fn leaf_outputs_match_user_order() {
    let users = make_users(8, 1_000);
    let root = build_tree(&users, funding_outpoint()).unwrap();

    let leaf_pairs = collect_leaf_outputs(&root);

    // Flatten pairs into user order
    let mut leaf_users: Vec<&User> = Vec::new();
    for (l, r) in &leaf_pairs {
        leaf_users.push(l);
        leaf_users.push(r);
    }

    assert_eq!(leaf_users.len(), users.len());

    for (i, (leaf_user, input_user)) in leaf_users.iter().zip(users.iter()).enumerate() {
        assert_eq!(
            leaf_user.pubkey, input_user.pubkey,
            "user {i} pubkey mismatch"
        );
        assert_eq!(
            leaf_user.amount, input_user.amount,
            "user {i} amount mismatch"
        );
    }
}

/// Every user's branch has the correct length (depth + 1 for internal nodes,
/// or just 1 for a 2-user tree which is a single leaf).
#[test]
fn branch_lengths_match_depth() {
    for n in [2, 4, 8, 16, 32, 64] {
        let root = build_tree(&make_users(n, 100), funding_outpoint()).unwrap();
        let expected_depth = root.depth();
        // Branch = root-to-leaf path = depth + 1 transactions
        // (for 2 users, depth=0, branch=1 tx)
        let expected_len = expected_depth + 1;

        for i in 0..n {
            let branch = root.branch(i).expect("branch must exist");
            assert_eq!(
                branch.len(),
                expected_len,
                "user {i} in {n}-user tree: branch len should be {expected_len}"
            );
        }

        // Out of range
        assert!(root.branch(n).is_none());
    }
}

/// Two trees built from the same users and funding outpoint must be identical.
#[test]
fn deterministic_construction() {
    let users = make_users(16, 1_234);
    let fp = funding_outpoint();
    let root1 = build_tree(&users, fp).unwrap();
    let root2 = build_tree(&users, fp).unwrap();

    let txs1 = collect_all_txs(&root1);
    let txs2 = collect_all_txs(&root2);

    assert_eq!(txs1.len(), txs2.len());
    for (t1, t2) in txs1.iter().zip(txs2.iter()) {
        assert_eq!(t1.compute_txid(), t2.compute_txid());
    }
}

/// Changing any single user's key produces a completely different tree
/// (different root txid).
#[test]
fn different_users_produce_different_tree() {
    let users_a = make_users(8, 1_000);
    let users_b = {
        let mut u = users_a.clone();
        u[3] = make_user(200, 1_000); // change user 3
        u
    };

    let fp = funding_outpoint();
    let root_a = build_tree(&users_a, fp).unwrap();
    let root_b = build_tree(&users_b, fp).unwrap();

    assert_ne!(
        root_a.tx().compute_txid(),
        root_b.tx().compute_txid(),
        "different users must produce different root txid"
    );
}

/// 1024-user tree: full-scale structure verification.
#[test]
fn full_scale_1024_users() {
    let n = 1024;
    let sats_each = 500u64;
    let users = make_users(n, sats_each);
    let root = build_tree(&users, funding_outpoint()).unwrap();

    // Structure
    assert_eq!(root.user_count(), 1024);
    assert_eq!(root.tx_count(), 1023);
    assert_eq!(root.depth(), 9);

    // Value
    assert_eq!(root.value(), Amount::from_sat(sats_each * n as u64));

    // Every user has a branch of length 10 (depth 9 + 1)
    let branch_first = root.branch(0).unwrap();
    let branch_last = root.branch(1023).unwrap();
    assert_eq!(branch_first.len(), 10);
    assert_eq!(branch_last.len(), 10);

    // First and last users' branches share only the root tx
    assert_eq!(
        branch_first[0].compute_txid(),
        branch_last[0].compute_txid(),
        "both branches start at the root"
    );
    assert_ne!(
        branch_first[1].compute_txid(),
        branch_last[1].compute_txid(),
        "branches diverge at level 1"
    );
}
