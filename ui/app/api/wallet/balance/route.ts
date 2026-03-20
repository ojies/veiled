// GET /api/wallet/balance?name=X — Get wallet balance

import { NextResponse } from "next/server";
import { getBalance } from "@/lib/wallet";

export async function GET(request: Request) {
  try {
    const { searchParams } = new URL(request.url);
    const name = searchParams.get("name");

    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    const result = await getBalance(name);
    return NextResponse.json(result);
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
