// POST /api/wallet/faucet — Mine regtest blocks to fund wallets

import { NextResponse } from "next/server";
import { faucet, getBalance, createWallet } from "@/lib/wallet";
import { MIN_FAUCET_SATS } from "@/lib/config";

export async function POST(request: Request) {
  try {
    const { names } = await request.json();

    if (!names || !Array.isArray(names) || names.length === 0) {
      return NextResponse.json(
        { error: "names[] required" },
        { status: 400 }
      );
    }

    // Ensure a miner wallet exists for maturing coinbases
    const miner = createWallet("faucet-miner");
    const results: Record<string, any> = {};

    for (const name of names) {
      try {
        const wallet = createWallet(name); // idempotent
        // Mine 1 block to this wallet's address
        faucet(wallet.address, 1);
        results[name] = { address: wallet.address, funded: true };
      } catch (e: any) {
        results[name] = { error: e.message };
      }
    }

    // Mine 101 blocks to mature all coinbases (coinbase needs 100 confirmations)
    faucet(miner.address, 101);

    // Fetch updated balances; if below minimum, mine more until threshold is met
    for (const name of names) {
      if (results[name]?.funded) {
        try {
          let bal = getBalance(name);
          while (bal.confirmed < MIN_FAUCET_SATS) {
            const wallet = createWallet(name);
            faucet(wallet.address, 1);
            faucet(miner.address, 101); // mature the new coinbase
            bal = getBalance(name);
          }
          results[name].balance = bal;
        } catch {
          // ignore
        }
      }
    }

    return NextResponse.json({ results });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
