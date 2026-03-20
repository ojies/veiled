// POST /api/wallet/create — Create a BDK wallet for any participant.
// BDK manages wallets locally — no bitcoind wallet needed.
// The Rust binary is idempotent: if the state file exists, it returns the existing wallet.

import { NextResponse } from "next/server";
import { createWallet, walletExists } from "@/lib/wallet";
import { setWallet, getWallet } from "@/lib/state";

export async function POST(request: Request) {
  try {
    const { name, role } = await request.json();
    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    // If wallet is already in memory, return it
    if (walletExists(name)) {
      const existing = getWallet(name);
      if (existing) {
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

    setWallet(name, {
      address: result.address,
      balance: 0,
      mnemonic: result.mnemonic,
      role: role || "beneficiary",
    });

    return NextResponse.json(result);
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
