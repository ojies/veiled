// GET /api/wallet/balance?name=X — Get wallet balance
// Uses scantxoutset for instant results (no BDK block-by-block sync).
// Falls back to cached balance from the wallet state file on error.

import { NextResponse } from "next/server";
import { getBalanceFast, getCachedBalance } from "@/lib/wallet";
import { log, logError } from "@/lib/log";

export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const name = searchParams.get("name");

  if (!name) {
    return NextResponse.json({ error: "name required" }, { status: 400 });
  }

  try {
    const result = await getBalanceFast(name);
    log("wallet/balance", `${name}: ${result.confirmed} confirmed, ${result.total} total`);
    return NextResponse.json(result);
  } catch (err: any) {
    // Fall back to cached balance so the UI doesn't show 0 during transient failures
    const cached = getCachedBalance(name);
    if (cached) {
      logError("wallet/balance", `scantxoutset failed for '${name}', using cache`, err);
      return NextResponse.json({ ...cached, cached: true });
    }
    logError("wallet/balance", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
