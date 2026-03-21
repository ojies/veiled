// GET /api/wallet/balance?name=X — Get wallet balance
// Uses scantxoutset for instant results (no BDK block-by-block sync).

import { NextResponse } from "next/server";
import { getBalanceFast } from "@/lib/wallet";
import { log, logError } from "@/lib/log";

export async function GET(request: Request) {
  try {
    const { searchParams } = new URL(request.url);
    const name = searchParams.get("name");

    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    const result = await getBalanceFast(name);
    log("wallet/balance", `${name}: ${result.confirmed} confirmed, ${result.total} total`);
    return NextResponse.json(result);
  } catch (err: any) {
    logError("wallet/balance", `failed for '${searchParams.get("name")}'`, err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
