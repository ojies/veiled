// GET /api/merchant/payments?merchant=CoffeeCo — payment history for a merchant

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
  const payments: {
    beneficiary: string;
    amount: number;
    address: string;
  }[] = [];

  for (const [name, ben] of Object.entries(state.beneficiaries)) {
    for (const pay of ben.payments) {
      if (pay.merchant_name === merchant) {
        payments.push({
          beneficiary: name,
          amount: pay.amount,
          address: pay.address,
        });
      }
    }
  }

  return NextResponse.json({ merchant, payments });
}
