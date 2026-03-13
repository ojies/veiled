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
  const identities: {
    beneficiary: string;
    pseudonym: string;
    nullifier: string;
  }[] = [];

  for (const [name, ben] of Object.entries(state.beneficiaries)) {
    for (const reg of ben.registrations) {
      if (reg.merchant_name === merchant && reg.status === "verified") {
        identities.push({
          beneficiary: name,
          pseudonym: reg.pseudonym,
          nullifier: reg.nullifier,
        });
      }
    }
  }

  return NextResponse.json({ merchant, identities });
}
