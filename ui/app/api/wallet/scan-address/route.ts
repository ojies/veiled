// GET /api/wallet/scan-address?address=bcrt1p... — check UTXOs at arbitrary address

import { NextResponse } from "next/server";

const RPC_URL = process.env.BITCOIN_RPC_URL || "http://localhost:18443";
const RPC_USER = process.env.BITCOIN_RPC_USER || "veiled";
const RPC_PASS = process.env.BITCOIN_RPC_PASS || "veiled";

export async function GET(request: Request) {
  const address = new URL(request.url).searchParams.get("address");
  if (!address) {
    return NextResponse.json({ error: "address required" }, { status: 400 });
  }

  const res = await fetch(RPC_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization:
        "Basic " + Buffer.from(`${RPC_USER}:${RPC_PASS}`).toString("base64"),
    },
    body: JSON.stringify({
      jsonrpc: "1.0",
      id: "scan",
      method: "scantxoutset",
      params: ["start", [`addr(${address})`]],
    }),
  });

  const data = await res.json();
  if (data.error) {
    return NextResponse.json({ error: data.error.message }, { status: 500 });
  }

  const sats = Math.round((data.result?.total_amount ?? 0) * 1e8);
  return NextResponse.json({
    total_amount_sats: sats,
    utxos: data.result?.unspents ?? [],
  });
}
