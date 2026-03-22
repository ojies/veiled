// GET /api/beneficiary/merchants — list available merchants

import { NextResponse } from "next/server";
import { getState } from "@/lib/state";

export async function GET() {
  const state = getState();
  return NextResponse.json({ merchants: state.merchants });
}
