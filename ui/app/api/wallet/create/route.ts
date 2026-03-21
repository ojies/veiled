// POST /api/wallet/create — Create a BDK wallet for any participant.
// BDK manages wallets locally — no bitcoind wallet needed.
// The Rust binary is idempotent: if the state file exists, it returns the existing wallet.

import { NextResponse } from "next/server";
import { createWallet, walletExists } from "@/lib/wallet";
import { setWallet, getWallet } from "@/lib/state";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { name, role } = await request.json();
    log("wallet/create", `name='${name}', role='${role}'`);
    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    // If wallet is already in memory, return it
    if (walletExists(name)) {
      const existing = getWallet(name);
      if (existing) {
        log("wallet/create", `already exists in memory: ${existing.address}`);
        return NextResponse.json({
          address: existing.address,
          mnemonic: existing.mnemonic,
          wallet_name: name,
          existing: true,
        });
      }
    }

    // Create wallet (or load existing from state file — handled by Rust binary)
    const result = await createWallet(name);
    log("wallet/create", `created: ${result.address}`, { existing: result.existing });

    setWallet(name, {
      address: result.address,
      balance: 0,
      mnemonic: result.mnemonic,
      role: role || "beneficiary",
    });

    return NextResponse.json(result);
  } catch (err: any) {
    logError("wallet/create", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
