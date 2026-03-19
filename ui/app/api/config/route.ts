import { NextResponse } from "next/server";
import { MIN_MERCHANTS, BENEFICIARY_CAPACITY } from "@/lib/config";

export async function GET() {
  return NextResponse.json({ minMerchants: MIN_MERCHANTS, beneficiaryCapacity: BENEFICIARY_CAPACITY });
}
