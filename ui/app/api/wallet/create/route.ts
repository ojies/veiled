// POST /api/wallet/create — Create a BDK wallet for any participant
// If the wallet already exists (e.g. from a previous session), loads it instead.

import { NextResponse } from "next/server";
import { createWallet, walletExists, getAddress } from "@/lib/wallet";
import { setWallet, getWallet } from "@/lib/state";

export async function POST(request: Request) {
  try {
    const { name, role } = await request.json();
    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    // If wallet state file already exists, return the existing wallet
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
      // State file exists but not in memory — reload address
      try {
        const addr = getAddress(name);
        const info = {
          address: addr.address,
          balance: 0,
          mnemonic: "",
          role: role || "beneficiary",
        };
        setWallet(name, info);
        return NextResponse.json({ ...addr, wallet_name: name, existing: true });
      } catch {
        // Fall through to create
      }
    }

    let result;
    try {
      result = createWallet(name);
    } catch (err: any) {
      // If bitcoind says the wallet already exists, try loading it
      if (err.message?.includes("already exists") || err.message?.includes("Database already")) {
        try {
          const addr = getAddress(name);
          const info = {
            address: addr.address,
            balance: 0,
            mnemonic: "",
            role: role || "beneficiary",
          };
          setWallet(name, info);
          return NextResponse.json({ ...addr, wallet_name: name, existing: true });
        } catch (loadErr: any) {
          return NextResponse.json({ error: loadErr.message }, { status: 500 });
        }
      }
      throw err;
    }

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
