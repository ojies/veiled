use bitcoin::{Amount, OutPoint};

use crate::tx::{aggregate_keys, create_internal_tx, create_leaf_tx};
use crate::types::{TreeNode, User};

/// Builds a binary transaction tree from a list of users.
///
/// The users are paired left-to-right into leaf transactions.
/// Leaf transactions are then paired into internal transactions,
/// and so on until a single root node remains.
///
/// # Requirements
/// - `users.len()` must be a power of 2 and >= 2.
/// - `funding_outpoint` is the on-chain UTXO that funds the root transaction.
///
/// # Returns
/// The root `TreeNode` of the fully constructed tree.
pub fn build_tree(users: &[User], funding_outpoint: OutPoint) -> Result<TreeNode, &'static str> {
    build_tree_with_fee(users, funding_outpoint, Amount::ZERO)
}

/// Like `build_tree`, but each internal transaction's outputs include
/// extra value (`fee_per_node`) so that child transactions can pay a
/// mining fee when broadcast.
///
/// The fee is deducted from the child's input → output difference.
/// Each parent output = sum(descendant_user_amounts) + fee_per_node * descendant_internal_nodes.
pub fn build_tree_with_fee(
    users: &[User],
    funding_outpoint: OutPoint,
    fee_per_node: Amount,
) -> Result<TreeNode, &'static str> {
    let n = users.len();
    if n < 2 {
        return Err("need at least 2 users");
    }
    if !n.is_power_of_two() {
        return Err("user count must be a power of 2");
    }

    // Phase 1: build leaf nodes with placeholder outpoints.
    // We'll fix the outpoints in phase 2 once we know parent txids.
    let mut current_level: Vec<TreeNode> = users
        .chunks_exact(2)
        .map(|pair| {
            let left = &pair[0];
            let right = &pair[1];
            let (left_xonly, _) = left.pubkey.x_only_public_key();
            let (right_xonly, _) = right.pubkey.x_only_public_key();

            // Placeholder outpoint — will be replaced when parent is built.
            let placeholder = OutPoint::null();

            let tx = create_leaf_tx(
                placeholder,
                &left_xonly,
                left.amount,
                &right_xonly,
                right.amount,
            );

            TreeNode::Leaf {
                tx,
                left_user: left.clone(),
                right_user: right.clone(),
            }
        })
        .collect();

    // Phase 2: build internal levels bottom-up.
    // Each internal tx output must cover the child's total output sum
    // plus the fee the child will need when broadcast.
    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len() / 2);
        let mut pairs = current_level.into_iter();

        while let (Some(left), Some(right)) = (pairs.next(), pairs.next()) {
            // value() returns the user-amount sum (excludes fee budget).
            // For the parent output we need: child_output_sum + fee_per_node
            // so the child tx has `fee_per_node` available as mining fee.
            let left_output_value = left.value_with_fees();
            let right_output_value = right.value_with_fees();
            let left_parent_output = left_output_value + fee_per_node;
            let right_parent_output = right_output_value + fee_per_node;

            let left_keys = collect_user_keys(&left);
            let right_keys = collect_user_keys(&right);
            let left_agg = aggregate_keys(&left_keys);
            let right_agg = aggregate_keys(&right_keys);

            let mut all_keys = left_keys;
            all_keys.extend(right_keys);
            let node_agg = aggregate_keys(&all_keys);

            let placeholder = OutPoint::null();

            let tx = create_internal_tx(
                placeholder,
                &left_agg,
                left_parent_output,
                &right_agg,
                right_parent_output,
            );

            let total_value = left_parent_output + right_parent_output;

            next_level.push(TreeNode::Internal {
                tx,
                aggregate_key: node_agg,
                value: total_value,
                left: Box::new(left),
                right: Box::new(right),
            });
        }

        current_level = next_level;
    }

    // Phase 3: the single remaining node is the root.
    // Set the root's input to the funding outpoint, then propagate
    // correct outpoints down the tree.
    let mut root = current_level.into_iter().next().unwrap();
    set_input_outpoint(&mut root, funding_outpoint);
    propagate_outpoints(&mut root);

    Ok(root)
}

/// Collects all user public keys under a node.
fn collect_user_keys(node: &TreeNode) -> Vec<bitcoin::secp256k1::PublicKey> {
    match node {
        TreeNode::Leaf {
            left_user,
            right_user,
            ..
        } => {
            vec![left_user.pubkey, right_user.pubkey]
        }
        TreeNode::Internal { left, right, .. } => {
            let mut keys = collect_user_keys(left);
            keys.extend(collect_user_keys(right));
            keys
        }
    }
}

/// Sets the input outpoint of a node's transaction.
fn set_input_outpoint(node: &mut TreeNode, outpoint: OutPoint) {
    match node {
        TreeNode::Internal { tx, .. } | TreeNode::Leaf { tx, .. } => {
            tx.input[0].previous_output = outpoint;
        }
    }
}

/// After the tree is built, propagate correct outpoints from parent to children.
///
/// Each child's input must reference the correct output of its parent transaction.
/// Left child spends output 0, right child spends output 1.
fn propagate_outpoints(node: &mut TreeNode) {
    if let TreeNode::Internal {
        tx, left, right, ..
    } = node
    {
        let txid = tx.compute_txid();

        // Left child spends output 0
        set_input_outpoint(left, OutPoint { txid, vout: 0 });
        propagate_outpoints(left);

        // Right child spends output 1
        set_input_outpoint(right, OutPoint { txid, vout: 1 });
        propagate_outpoints(right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
    use bitcoin::Amount;

    fn make_user(seed: u8, sats: u64) -> User {
        let secp = Secp256k1::new();
        let mut secret = [0u8; 32];
        secret[31] = seed;
        if seed == 0 {
            secret[31] = 1; // 0 is not a valid secret key
        }
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

    #[test]
    fn tree_2_users() {
        let users = make_users(2, 10_000);
        let root = build_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(root.user_count(), 2);
        assert_eq!(root.tx_count(), 1);
        assert_eq!(root.depth(), 0);
        assert_eq!(root.value(), Amount::from_sat(20_000));

        // It's a leaf — tx has 2 outputs
        assert_eq!(root.tx().output.len(), 2);
        assert_eq!(root.tx().output[0].value, Amount::from_sat(10_000));
        assert_eq!(root.tx().output[1].value, Amount::from_sat(10_000));
    }

    #[test]
    fn tree_4_users() {
        let users = make_users(4, 5_000);
        let root = build_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(root.user_count(), 4);
        assert_eq!(root.tx_count(), 3); // 1 root + 2 leaves
        assert_eq!(root.depth(), 1);
        assert_eq!(root.value(), Amount::from_sat(20_000));

        // Root is internal with 2 outputs
        assert_eq!(root.tx().output.len(), 2);
        assert_eq!(root.tx().output[0].value, Amount::from_sat(10_000));
        assert_eq!(root.tx().output[1].value, Amount::from_sat(10_000));

        // Children should reference root's outputs
        if let TreeNode::Internal { left, right, tx, .. } = &root {
            let root_txid = tx.compute_txid();
            assert_eq!(left.tx().input[0].previous_output.txid, root_txid);
            assert_eq!(left.tx().input[0].previous_output.vout, 0);
            assert_eq!(right.tx().input[0].previous_output.txid, root_txid);
            assert_eq!(right.tx().input[0].previous_output.vout, 1);
        } else {
            panic!("root should be Internal for 4 users");
        }
    }

    #[test]
    fn tree_1024_users() {
        let users = make_users(1024, 1_000);
        let root = build_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(root.user_count(), 1024);
        assert_eq!(root.tx_count(), 1023); // 2^0 + 2^1 + ... + 2^9
        assert_eq!(root.depth(), 9); // log2(1024) - 1 = 9 internal levels
        assert_eq!(root.value(), Amount::from_sat(1_024_000));
    }

    #[test]
    fn branch_length() {
        let users = make_users(8, 1_000);
        let root = build_tree(&users, OutPoint::null()).unwrap();

        // depth = 2 (8 users → 4 leaves → 2 internals → 1 root)
        // branch from root to leaf = 3 transactions (root + 1 internal + 1 leaf)
        let branch = root.branch(0).unwrap();
        assert_eq!(branch.len(), 3);

        let branch = root.branch(7).unwrap();
        assert_eq!(branch.len(), 3);

        // Out-of-range returns None
        assert!(root.branch(8).is_none());
    }

    #[test]
    fn branch_outpoints_chain_correctly() {
        let users = make_users(4, 5_000);
        let root = build_tree(&users, OutPoint::null()).unwrap();

        // User 0's branch: root → left leaf
        let branch = root.branch(0).unwrap();
        assert_eq!(branch.len(), 2);

        // The leaf's input should spend the root's output 0
        let root_txid = branch[0].compute_txid();
        assert_eq!(branch[1].input[0].previous_output.txid, root_txid);
        assert_eq!(branch[1].input[0].previous_output.vout, 0);
    }

    #[test]
    fn rejects_non_power_of_two() {
        let users = make_users(3, 1_000);
        assert!(build_tree(&users, OutPoint::null()).is_err());
    }

    #[test]
    fn rejects_single_user() {
        let users = make_users(1, 1_000);
        assert!(build_tree(&users, OutPoint::null()).is_err());
    }

    #[test]
    fn different_amounts_per_user() {
        let users = vec![make_user(1, 3_000), make_user(2, 7_000)];
        let root = build_tree(&users, OutPoint::null()).unwrap();

        assert_eq!(root.tx().output[0].value, Amount::from_sat(3_000));
        assert_eq!(root.tx().output[1].value, Amount::from_sat(7_000));
        assert_eq!(root.value(), Amount::from_sat(10_000));
    }
}
