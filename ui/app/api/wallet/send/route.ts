// POST /api/wallet/send — Send BTC from one wallet to an address

import { NextResponse } from "next/server";
import { send } from "@/lib/wallet";

export async function POST(request: Request) {
  try {
    const { from, to_address, amount_sats } = await request.json();

    if (!from || !to_address || !amount_sats) {
      return NextResponse.json(
        { error: "from, to_address, and amount_sats required" },
        { status: 400 }
      );
    }

    const result = send(from, to_address, amount_sats);
    return NextResponse.json(result);
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
