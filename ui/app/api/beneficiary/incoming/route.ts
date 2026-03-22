// GET /api/beneficiary/incoming?name=X — Check for incoming payments to a beneficiary

import { NextResponse } from "next/server";
import { getTxHistory } from "@/lib/wallet";

export async function GET(request: Request) {
  try {
    const { searchParams } = new URL(request.url);
    const name = searchParams.get("name");

    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    // Wallet name for beneficiary
    const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;

    const history = await getTxHistory(walletName);

    // Filter to incoming transactions only
    const incoming = (history.transactions || []).filter(
      (tx: any) => tx.direction === "incoming"
    );

    return NextResponse.json({
      name,
      transactions: incoming,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
