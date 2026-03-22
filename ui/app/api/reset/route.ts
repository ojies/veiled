// POST /api/reset — reset simulation state (UI + registry gRPC)

import { NextResponse } from "next/server";
import { resetState } from "@/lib/state";
import { getRegistryClient, resetClients, grpcCall } from "@/lib/grpc";
import path from "path";
import fs from "fs";

const WALLETS_DIR =
  process.env.WALLETS_DIR || path.resolve(process.cwd(), "../.wallets");

export async function POST() {
  // Reset registry gRPC state first (clears merchants, sets, beneficiaries)
  try {
    const registry = getRegistryClient();
    await grpcCall(registry, "Reset", {});
  } catch (_e) {
    // Registry may not be running — UI reset still proceeds
  }

  // Reset ALL wallet BDK checkpoints to null so the next sync rescans from
  // genesis. This is necessary because:
  // 1. At high block heights, regtest subsidy shrinks (halvings every 150
  //    blocks) and new coinbases alone may not cover faucet/fee amounts.
  //    A full rescan finds all accumulated UTXOs across the chain.
  // 2. Merchant/beneficiary wallets from prior runs have stale checkpoints.
  //    Without resetting, their send() calls sync only from the old height
  //    and miss UTXOs (e.g., faucet deposits) confirmed before that height.
  try {
    const entries = fs.readdirSync(WALLETS_DIR);
    for (const entry of entries) {
      if (!entry.endsWith(".json")) continue;
      const walletPath = path.join(WALLETS_DIR, entry);
      try {
        const state = JSON.parse(fs.readFileSync(walletPath, "utf8"));
        state.checkpoint_height = null;
        state.checkpoint_hash = null;
        state.cached_balance = null;
        fs.writeFileSync(walletPath, JSON.stringify(state, null, 2));
      } catch (_e) { /* non-critical per wallet */ }
    }
  } catch (_e) { /* wallets dir may not exist */ }

  resetState();
  resetClients();
  return NextResponse.json({ ok: true });
}
