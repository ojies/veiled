// TypeScript wrapper for the veiled-wallet Rust binary.
// Uses a persistent daemon process for fast IPC.

import path from "path";
import fs from "fs";
import { ProcessPool } from "./process-pool";

const WALLET_BIN =
  process.env.WALLET_BIN ||
  path.resolve(process.cwd(), "../target/release/veiled-wallet");
const WALLETS_DIR =
  process.env.WALLETS_DIR || path.resolve(process.cwd(), "../.wallets");

// Ensure wallets directory exists
if (!fs.existsSync(WALLETS_DIR)) {
  fs.mkdirSync(WALLETS_DIR, { recursive: true });
}

const RPC_URL = process.env.BITCOIN_RPC_URL || "http://localhost:18443";
const RPC_USER = process.env.BITCOIN_RPC_USER || "veiled";
const RPC_PASS = process.env.BITCOIN_RPC_PASS || "veiled";

const pool = new ProcessPool(WALLET_BIN, ["--daemon"], 30000);

async function callWallet(command: string, params: Record<string, unknown> = {}): Promise<any> {
  return pool.call({ command, ...params });
}

function statePath(name: string): string {
  return path.join(WALLETS_DIR, `${name}.json`);
}

export function walletExists(name: string): boolean {
  return fs.existsSync(statePath(name));
}

export async function createWallet(name: string) {
  return callWallet("create-wallet", {
    state_path: statePath(name),
    name,
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

/** Read cached balance from wallet state file (instant, no daemon call). */
export function getCachedBalance(name: string): { confirmed: number; unconfirmed: number; total: number } | null {
  const p = statePath(name);
  if (!fs.existsSync(p)) return null;
  try {
    const data = JSON.parse(fs.readFileSync(p, "utf8"));
    return data.cached_balance || null;
  } catch {
    return null;
  }
}

/** Fast balance via scantxoutset (no BDK sync, queries UTXO set directly). */
export async function getBalanceFast(name: string) {
  return callWallet("get-balance-fast", {
    state_path: statePath(name),
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

/** Full balance via BDK sync (slower, updates cache). */
export async function getBalance(name: string) {
  return callWallet("get-balance", {
    state_path: statePath(name),
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

export async function getAddress(name: string) {
  return callWallet("get-address", {
    state_path: statePath(name),
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

export async function send(fromName: string, toAddress: string, amountSats: number) {
  return callWallet("send", {
    state_path: statePath(fromName),
    to_address: toAddress,
    amount_sats: amountSats,
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

export async function faucet(address: string, blocks?: number) {
  return callWallet("faucet", {
    address,
    blocks: blocks || 1,
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

export async function getTx(txid: string) {
  return callWallet("get-tx", {
    txid,
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}

export async function getTxHistory(name: string) {
  return callWallet("get-tx-history", {
    state_path: statePath(name),
    rpc_url: RPC_URL,
    rpc_user: RPC_USER,
    rpc_pass: RPC_PASS,
  });
}
