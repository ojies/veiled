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
  const proc = state.merchant_processes[merchant];
  const payments = (proc?.pending_payments ?? []).map((p) => ({
    beneficiary: p.beneficiary,
    amount: p.amount,
    address: p.address,
  }));

  return NextResponse.json({ merchant, payments });
}
