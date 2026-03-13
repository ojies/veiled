// POST /api/wallet/create — Create a BDK wallet for any participant

import { NextResponse } from "next/server";
import { createWallet } from "@/lib/wallet";
import { setWallet } from "@/lib/state";

export async function POST(request: Request) {
  try {
    const { name, role } = await request.json();
    if (!name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }

    const result = createWallet(name);

    setWallet(name, {
      address: result.address,
      balance: 0,
      mnemonic: result.mnemonic,
      role: role || "beneficiary",
    });

    return NextResponse.json(result);
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
