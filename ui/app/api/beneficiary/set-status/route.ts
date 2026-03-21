// GET /api/beneficiary/set-status — Lightweight poll for anonymity set status
// Returns { count, capacity, finalized } without modifying state.

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState } from "@/lib/state";

export async function GET() {
  try {
    const state = getState();
    const registry = getRegistryClient();
    const resp: any = await grpcCall(registry, "GetAnonymitySet", {
      set_id: state.set_id,
    });
    return NextResponse.json({
      count: resp.count,
      capacity: resp.capacity,
      finalized: resp.finalized,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
