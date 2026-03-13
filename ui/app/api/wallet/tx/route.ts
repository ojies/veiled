// GET /api/wallet/tx?txid=X — Get transaction details

import { NextResponse } from "next/server";
import { getTx } from "@/lib/wallet";

export async function GET(request: Request) {
  try {
    const { searchParams } = new URL(request.url);
    const txid = searchParams.get("txid");

    if (!txid) {
      return NextResponse.json({ error: "txid required" }, { status: 400 });
    }

    const result = getTx(txid);
    return NextResponse.json(result);
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
