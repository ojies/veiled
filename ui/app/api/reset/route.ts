// POST /api/reset — reset simulation state

import { NextResponse } from "next/server";
import { resetState } from "@/lib/state";
import { resetClients } from "@/lib/grpc";

export async function POST() {
  resetState();
  resetClients();
  return NextResponse.json({ ok: true });
}
