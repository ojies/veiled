// GET /api/merchant/identities?merchant=CoffeeCo — registered identities for a merchant

import { NextResponse } from "next/server";
import { getState } from "@/lib/state";

export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const merchant = searchParams.get("merchant");

  if (!merchant) {
    return NextResponse.json(
      { error: "merchant query param required" },
      { status: 400 }
    );
  }

  const state = getState();
  const proc = state.merchant_processes[merchant];
  const identities = (proc?.registered_identities ?? []).map((id) => ({
    beneficiary: id.friendly_name,
    pseudonym: id.pseudonym,
    nullifier: id.nullifier,
  }));

  return NextResponse.json({ merchant, identities });
}
