//! End-to-end tests against a Bitcoin Core regtest node.
//!
//! Prerequisites:
//!   docker compose up -d   (from project root, if docker-compose.yml present)
//!
//! Run:
//!   cargo test --test e2e -- --ignored --test-threads=1
//!
//! The tests are `#[ignore]` by default so `cargo test` doesn't fail
//! when bitcoind isn't running.  Use `--test-threads=1` to avoid wallet
//! lock contention.

use bitcoin::secp256k1::{Keypair, PublicKey, Secp256k1, SecretKey};
use bitcoin::{Address, Amount, Network, OutPoint, TxOut};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use veiled::vtxo_tree::tree::{build_tree, build_tree_with_fee};
use veiled::vtxo_tree::tx::{aggregate_secret_key, p2tr_script, sign_tx};
use veiled::vtxo_tree::types::{TreeNode, User};

// ── helpers ──────────────────────────────────────────────────────────────────

const RPC_URL: &str = "http://localhost:18443";
const RPC_USER: &str = "vtxo";
const RPC_PASS: &str = "vtxo";
const WALLET: &str = "vtxo_e2e";

fn rpc_client() -> Client {
    Client::new(RPC_URL, Auth::UserPass(RPC_USER.into(), RPC_PASS.into())).expect("RPC connect")
}

/// Ensure a wallet exists and is loaded, then return the client.
fn setup_wallet(rpc: &Client) {
    let wallets: Vec<String> = rpc.list_wallets().unwrap_or_default();
    if wallets.contains(&WALLET.to_string()) {
        return;
    }
    // Try loading first, then create if not found.
    if rpc.load_wallet(WALLET).is_ok() {
        return;
    }
    // Create wallet — ignore "already exists" errors from race conditions.
    let _ = rpc.create_wallet(WALLET, None, None, None, None);
}

fn wallet_rpc() -> Client {
    let url = format!("{}/wallet/{}", RPC_URL, WALLET);
    Client::new(&url, Auth::UserPass(RPC_USER.into(), RPC_PASS.into())).expect("wallet RPC")
}

/// Mine blocks to a new address in the wallet and return the address used.
fn mine_blocks(rpc: &Client, n: u64) -> Address {
    let addr = rpc
        .get_new_address(None, Some(bitcoincore_rpc::json::AddressType::Bech32m))
        .expect("get address")
        .require_network(Network::Regtest)
        .expect("regtest address");
    rpc.generate_to_address(n, &addr).expect("mine blocks");
    addr
}

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

/// Collect all user pubkeys under a node (used to derive the aggregate secret key).
fn collect_user_keys(node: &TreeNode) -> Vec<PublicKey> {
    match node {
        TreeNode::Leaf {
            left_user,
            right_user,
            ..
        } => vec![left_user.pubkey, right_user.pubkey],
        TreeNode::Internal { left, right, .. } => {
            let mut keys = collect_user_keys(left);
            keys.extend(collect_user_keys(right));
            keys
        }
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Build a 4-user tree, fund it on regtest, broadcast the root tx, and
/// verify it confirms.
#[test]
#[ignore = "requires bitcoind regtest (docker compose up -d)"]
fn broadcast_root_tx() {
    let rpc = rpc_client();
    setup_wallet(&rpc);
    let wrpc = wallet_rpc();

    // Mine 101 blocks for coinbase maturity.
    mine_blocks(&wrpc, 101);

    // Create a funding tx: send to a known key we control.
    let secp = Secp256k1::new();
    let funding_sk = SecretKey::from_slice(&[0xAA; 32]).unwrap();
    let funding_kp = Keypair::from_secret_key(&secp, &funding_sk);
    let (funding_xonly, _) = funding_kp.x_only_public_key();
    let funding_script = p2tr_script(&funding_xonly);
    let funding_addr =
        Address::from_script(&funding_script, Network::Regtest).expect("valid P2TR address");

    // Send 50_000 sats to the funding address.
    let total_sats = 50_000u64;
    let funding_txid = wrpc
        .send_to_address(
            &funding_addr,
            Amount::from_sat(total_sats),
            None, None, None, None, None, None,
        )
        .expect("send to funding address");

    // Mine it.
    mine_blocks(&wrpc, 1);

    // Find the exact vout.
    let funding_tx_info = wrpc
        .get_raw_transaction_info(&funding_txid, None)
        .expect("get funding tx");
    let funding_vout = funding_tx_info
        .vout
        .iter()
        .position(|o| o.script_pub_key.script().expect("script") == funding_script)
        .expect("find funding vout") as u32;

    let funding_outpoint = OutPoint {
        txid: funding_txid,
        vout: funding_vout,
    };

    // Build a 4-user tree (3 txs total).
    // Each user gets 12_000 sats. Total = 48_000 (leaves 2_000 implicit fee).
    let users = make_users(4, 12_000);
    let mut root = build_tree(&users, funding_outpoint).unwrap();

    assert_eq!(root.tx_count(), 3);
    assert_eq!(root.user_count(), 4);

    // Sign the root tx with the funding key.
    let prev_txout = TxOut {
        value: Amount::from_sat(total_sats),
        script_pubkey: funding_script,
    };

    // The root tx is either Internal or Leaf (for 4 users it's Internal).
    sign_tx(root.tx_mut(), &funding_sk, &prev_txout);

    // Broadcast the root tx.
    let root_tx = root.tx().clone();
    let root_txid = wrpc
        .send_raw_transaction(&root_tx)
        .expect("broadcast root tx");

    // Mine a block to confirm it.
    mine_blocks(&wrpc, 1);

    // Verify it's confirmed.
    let tx_info = wrpc
        .get_raw_transaction_info(&root_txid, None)
        .expect("get root tx info");
    assert!(
        tx_info.confirmations.unwrap_or(0) >= 1,
        "root tx should have at least 1 confirmation"
    );

    // Verify it has 2 outputs with correct values.
    assert_eq!(tx_info.vout.len(), 2);
    let output_sum: f64 = tx_info.vout.iter().map(|o| o.value.to_btc()).sum();
    let expected_btc = Amount::from_sat(48_000).to_btc();
    assert!(
        (output_sum - expected_btc).abs() < 0.000_000_01,
        "output sum {output_sum} != expected {expected_btc}"
    );

    println!("Root tx confirmed: {root_txid}");
}

/// Build a 4-user tree, broadcast the root, then do a unilateral exit:
/// broadcast the left child tx (spending root output 0) to claim user 0
/// and user 1's funds.
#[test]
#[ignore = "requires bitcoind regtest (docker compose up -d)"]
fn unilateral_exit_child_tx() {
    let rpc = rpc_client();
    setup_wallet(&rpc);
    let wrpc = wallet_rpc();

    mine_blocks(&wrpc, 101);

    let secp = Secp256k1::new();
    let funding_sk = SecretKey::from_slice(&[0xBB; 32]).unwrap();
    let funding_kp = Keypair::from_secret_key(&secp, &funding_sk);
    let (funding_xonly, _) = funding_kp.x_only_public_key();
    let funding_script = p2tr_script(&funding_xonly);
    let funding_addr =
        Address::from_script(&funding_script, Network::Regtest).expect("valid P2TR address");

    let total_sats = 50_000u64;
    let funding_txid = wrpc
        .send_to_address(
            &funding_addr,
            Amount::from_sat(total_sats),
            None, None, None, None, None, None,
        )
        .expect("send to funding address");

    mine_blocks(&wrpc, 1);

    let funding_tx_info = wrpc
        .get_raw_transaction_info(&funding_txid, None)
        .expect("get funding tx");
    let funding_vout = funding_tx_info
        .vout
        .iter()
        .position(|o| o.script_pub_key.script().expect("script") == funding_script)
        .expect("find funding vout") as u32;

    let funding_outpoint = OutPoint {
        txid: funding_txid,
        vout: funding_vout,
    };

    let users = make_users(4, 12_000);
    // 300 sats per internal node covers the min relay fee for child txs.
    let fee_per_node = Amount::from_sat(300);
    let mut root = build_tree_with_fee(&users, funding_outpoint, fee_per_node).unwrap();

    // Sign and broadcast root tx.
    let prev_txout = TxOut {
        value: Amount::from_sat(total_sats),
        script_pubkey: funding_script,
    };
    sign_tx(root.tx_mut(), &funding_sk, &prev_txout);

    let root_tx = root.tx().clone();
    wrpc.send_raw_transaction(&root_tx)
        .expect("broadcast root tx");
    mine_blocks(&wrpc, 1);

    // Now do unilateral exit: broadcast the left child tx.
    // The left child spends root output 0.
    if let TreeNode::Internal {
        ref mut left,
        ref tx,
        ..
    } = root
    {
        // The left subtree's aggregate key is needed to sign.
        let left_keys = collect_user_keys(left);
        let left_agg_sk = aggregate_secret_key(&left_keys);

        // The prev output is root's output 0.
        let root_output_0 = tx.output[0].clone();

        sign_tx(left.tx_mut(), &left_agg_sk, &root_output_0);

        let left_tx = left.tx().clone();
        let left_txid = wrpc
            .send_raw_transaction(&left_tx)
            .expect("broadcast left child tx (unilateral exit)");

        mine_blocks(&wrpc, 1);

        let tx_info = wrpc
            .get_raw_transaction_info(&left_txid, None)
            .expect("get left child tx info");
        assert!(
            tx_info.confirmations.unwrap_or(0) >= 1,
            "left child tx should be confirmed"
        );

        // It should have 2 outputs (user 0 and user 1).
        assert_eq!(tx_info.vout.len(), 2);

        println!("Unilateral exit confirmed: {left_txid}");
    } else {
        panic!("root should be Internal for 4 users");
    }
}

/// Full branch exit: from root all the way to a leaf, claiming a single
/// user's output. Uses an 8-user tree (depth 2, branch length 3).
#[test]
#[ignore = "requires bitcoind regtest (docker compose up -d)"]
fn full_branch_exit_to_leaf() {
    let rpc = rpc_client();
    setup_wallet(&rpc);
    let wrpc = wallet_rpc();

    mine_blocks(&wrpc, 101);

    let secp = Secp256k1::new();
    let funding_sk = SecretKey::from_slice(&[0xCC; 32]).unwrap();
    let funding_kp = Keypair::from_secret_key(&secp, &funding_sk);
    let (funding_xonly, _) = funding_kp.x_only_public_key();
    let funding_script = p2tr_script(&funding_xonly);
    let funding_addr =
        Address::from_script(&funding_script, Network::Regtest).expect("valid P2TR address");

    let total_sats = 100_000u64;
    let funding_txid = wrpc
        .send_to_address(
            &funding_addr,
            Amount::from_sat(total_sats),
            None, None, None, None, None, None,
        )
        .expect("send to funding address");

    mine_blocks(&wrpc, 1);

    let funding_tx_info = wrpc
        .get_raw_transaction_info(&funding_txid, None)
        .expect("get funding tx");
    let funding_vout = funding_tx_info
        .vout
        .iter()
        .position(|o| o.script_pub_key.script().expect("script") == funding_script)
        .expect("find funding vout") as u32;

    let funding_outpoint = OutPoint {
        txid: funding_txid,
        vout: funding_vout,
    };

    // 8 users, 12_000 sats each = 96_000 user value.
    // Fee per node = 300 sats. 7 internal txs, so 2_100 total fee budget.
    // Root tx fee = 100_000 - root_output_sum.
    let users = make_users(8, 12_000);
    let fee_per_node = Amount::from_sat(300);
    let mut root = build_tree_with_fee(&users, funding_outpoint, fee_per_node).unwrap();

    assert_eq!(root.depth(), 2);
    assert_eq!(root.tx_count(), 7); // 1 + 2 + 4

    // Sign and broadcast root.
    let prev_txout = TxOut {
        value: Amount::from_sat(total_sats),
        script_pubkey: funding_script,
    };
    sign_tx(root.tx_mut(), &funding_sk, &prev_txout);
    wrpc.send_raw_transaction(root.tx())
        .expect("broadcast root");
    mine_blocks(&wrpc, 1);

    // Now broadcast the full branch for user 0: root → left internal → left leaf.
    // Each step: sign the child tx with the aggregate key, then broadcast.
    sign_and_broadcast_branch(&wrpc, &mut root, 0);

    // Verify the leaf tx is confirmed.
    let branch = root.branch(0).unwrap();
    let leaf_tx = branch.last().unwrap();
    let leaf_txid = leaf_tx.compute_txid();

    let tx_info = wrpc
        .get_raw_transaction_info(&leaf_txid, None)
        .expect("get leaf tx info");
    assert!(
        tx_info.confirmations.unwrap_or(0) >= 1,
        "leaf tx should be confirmed"
    );

    // Leaf tx has 2 outputs: user 0 (12_000) and user 1 (12_000).
    assert_eq!(tx_info.vout.len(), 2);

    println!(
        "Full branch exit for user 0 confirmed. Leaf txid: {leaf_txid}"
    );
}

/// Recursively sign and broadcast all transactions in a branch from root
/// (already broadcast) to the leaf containing `user_index`.
fn sign_and_broadcast_branch(rpc: &Client, node: &mut TreeNode, user_index: usize) {
    if let TreeNode::Internal {
        tx: parent_tx,
        left,
        right,
        ..
    } = node
    {
        let left_count = left.user_count();
        let (child, vout) = if user_index < left_count {
            (left.as_mut(), 0u32)
        } else {
            (right.as_mut(), 1u32)
        };

        // The child spends parent_tx.output[vout].
        let prev_output = parent_tx.output[vout as usize].clone();

        // Derive the signing key for the child.
        let child_keys = collect_user_keys(child);
        let child_sk = aggregate_secret_key(&child_keys);

        sign_tx(child.tx_mut(), &child_sk, &prev_output);

        rpc.send_raw_transaction(child.tx())
            .expect("broadcast child tx");
        mine_blocks(rpc, 1);

        // Recurse into the child's subtree.
        let child_user_index = if vout == 0 {
            user_index
        } else {
            user_index - left_count
        };
        sign_and_broadcast_branch(rpc, child, child_user_index);
    }
    // Leaf: nothing more to broadcast.
}

