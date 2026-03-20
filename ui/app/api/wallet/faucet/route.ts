// POST /api/wallet/faucet — Mine regtest blocks to fund wallets
//
// Accepts either:
//   { address: "bcrt1p..." }  — fund a specific address
//   { names: ["alice", ...] } — fund named wallets

import { NextResponse } from "next/server";
import { faucet, createWallet } from "@/lib/wallet";

export async function POST(request: Request) {
  try {
    const body = await request.json();

    // Ensure a miner wallet exists for maturing coinbases
    const miner = await createWallet("faucet-miner");

    // ── Fund by address ──
    if (body.address && typeof body.address === "string") {
      const addr = body.address.trim();
      const result = await faucet(addr, 10);
      await faucet(miner.address, 101);

      return NextResponse.json({
        address: addr,
        blocks_mined: result.blocks_mined,
        funded: true,
      });
    }

    // ── Fund by wallet names ──
    const { names } = body;
    if (!names || !Array.isArray(names) || names.length === 0) {
      return NextResponse.json(
        { error: "address (string) or names[] required" },
        { status: 400 }
      );
    }

    const results: Record<string, any> = {};

    for (const name of names) {
      try {
        const wallet = await createWallet(name); // idempotent
        await faucet(wallet.address, 10);
        results[name] = { address: wallet.address, funded: true };
      } catch (e: any) {
        results[name] = { error: e.message };
      }
    }

    // Mine 101 blocks to mature all coinbases (coinbase needs 100 confirmations)
    await faucet(miner.address, 101);

    return NextResponse.json({ results });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
