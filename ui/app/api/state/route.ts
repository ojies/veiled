// GET /api/state — full simulation state

import { NextResponse } from "next/server";
import { getState } from "@/lib/state";

export async function GET() {
  return NextResponse.json(getState());
}
