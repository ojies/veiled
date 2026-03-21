// POST /api/wallet/faucet — Fund wallets by sending from the pre-mined miner wallet
//
// Accepts either:
//   { address: "bcrt1p..." }  — send to a specific address
//   { names: ["alice", ...] } — send to named wallets

import { NextResponse } from "next/server";
import { faucet, send, createWallet, getAddress } from "@/lib/wallet";
import { FAUCET_AMOUNT_SATS } from "@/lib/config";
import { log, logError } from "@/lib/log";

const MINER_WALLET = "miner";

export async function POST(request: Request) {
  try {
    const body = await request.json();
    log("wallet/faucet", "request", body);

    // Get the miner address for confirmation mining
    const miner = await getAddress(MINER_WALLET);

    // ── Fund by address ──
    if (body.address && typeof body.address === "string") {
      const addr = body.address.trim();
      log("wallet/faucet", `funding address ${addr.slice(0, 20)}... with ${FAUCET_AMOUNT_SATS} sats`);
      await send(MINER_WALLET, addr, FAUCET_AMOUNT_SATS);
      await faucet(miner.address, 1);
      log("wallet/faucet", `funded ${addr.slice(0, 20)}... OK`);

      return NextResponse.json({
        address: addr,
        amount_sats: FAUCET_AMOUNT_SATS,
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
        await send(MINER_WALLET, wallet.address, FAUCET_AMOUNT_SATS);
        results[name] = { address: wallet.address, amount_sats: FAUCET_AMOUNT_SATS, funded: true };
      } catch (e: any) {
        results[name] = { error: e.message };
      }
    }

    // Mine 1 block to confirm all sends
    await faucet(miner.address, 1);

    return NextResponse.json({ results });
  } catch (err: any) {
    logError("wallet/faucet", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
