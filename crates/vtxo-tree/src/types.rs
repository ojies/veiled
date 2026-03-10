use bitcoin::secp256k1::PublicKey;
use bitcoin::{Amount, Transaction, XOnlyPublicKey};

/// A participant in the transaction tree.
#[derive(Debug, Clone)]
pub struct User {
    /// The user's public key (used for their leaf output).
    pub pubkey: PublicKey,
    /// The amount allocated to this user.
    pub amount: Amount,
}

/// A node in the binary transaction tree.
///
/// Internal nodes have a transaction that spends one parent output
/// and produces two child outputs.  Leaf nodes have a transaction
/// whose two outputs pay directly to two users.
#[derive(Debug, Clone)]
pub enum TreeNode {
    Internal {
        /// The pre-signed transaction at this node.
        tx: Transaction,
        /// Aggregate x-only key of all descendants (for Taproot output).
        aggregate_key: XOnlyPublicKey,
        /// Total value flowing through this node.
        value: Amount,
        left: Box<TreeNode>,
        right: Box<TreeNode>,
    },
    Leaf {
        /// The pre-signed transaction at this leaf.
        tx: Transaction,
        /// The two users whose outputs are in this leaf tx.
        left_user: User,
        right_user: User,
    },
}

impl TreeNode {
    /// Returns the transaction at this node.
    pub fn tx(&self) -> &Transaction {
        match self {
            TreeNode::Internal { tx, .. } => tx,
            TreeNode::Leaf { tx, .. } => tx,
        }
    }

    /// Returns a mutable reference to the transaction at this node.
    pub fn tx_mut(&mut self) -> &mut Transaction {
        match self {
            TreeNode::Internal { tx, .. } => tx,
            TreeNode::Leaf { tx, .. } => tx,
        }
    }

    /// Returns the total user-amount value at this node (excludes fee budget).
    pub fn value(&self) -> Amount {
        match self {
            TreeNode::Leaf {
                left_user,
                right_user,
                ..
            } => left_user.amount + right_user.amount,
            TreeNode::Internal { left, right, .. } => left.value() + right.value(),
        }
    }

    /// Returns the total output sum of this node's transaction
    /// (includes fee budgets baked into internal outputs).
    pub fn value_with_fees(&self) -> Amount {
        self.tx().output.iter().map(|o| o.value).sum()
    }

    /// Returns the number of users (leaf outputs) under this node.
    pub fn user_count(&self) -> usize {
        match self {
            TreeNode::Internal { left, right, .. } => {
                left.user_count() + right.user_count()
            }
            TreeNode::Leaf { .. } => 2,
        }
    }

    /// Returns the depth of the tree (0 for a leaf).
    pub fn depth(&self) -> usize {
        match self {
            TreeNode::Internal { left, .. } => 1 + left.depth(),
            TreeNode::Leaf { .. } => 0,
        }
    }

    /// Counts total transactions in the tree.
    pub fn tx_count(&self) -> usize {
        match self {
            TreeNode::Internal { left, right, .. } => {
                1 + left.tx_count() + right.tx_count()
            }
            TreeNode::Leaf { .. } => 1,
        }
    }

    /// Returns the branch (list of transactions) from root to a user's leaf.
    /// `user_index` is 0-based among all users.
    pub fn branch(&self, user_index: usize) -> Option<Vec<&Transaction>> {
        match self {
            TreeNode::Leaf { tx, .. } => {
                if user_index < 2 {
                    Some(vec![tx])
                } else {
                    None
                }
            }
            TreeNode::Internal {
                tx, left, right, ..
            } => {
                let left_count = left.user_count();
                let child_branch = if user_index < left_count {
                    left.branch(user_index)
                } else {
                    right.branch(user_index - left_count)
                };
                child_branch.map(|mut branch| {
                    branch.insert(0, tx);
                    branch
                })
            }
        }
    }
}
