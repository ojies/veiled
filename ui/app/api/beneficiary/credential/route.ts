// POST /api/beneficiary/credential — Phase 1: create credential locally

import { NextResponse } from "next/server";
import { createCredential } from "@/lib/core";
import { getState, addBeneficiary, setPhase } from "@/lib/state";

export async function POST(request: Request) {
  try {
    const { name } = await request.json();
    if (!name) {
      return NextResponse.json({ error: "name is required" }, { status: 400 });
    }

    const state = getState();
    if (!state.crs_hex) {
      return NextResponse.json(
        { error: "System not initialized — register at least one merchant first" },
        { status: 400 }
      );
    }

    if (state.beneficiaries[name]) {
      return NextResponse.json(
        { error: `beneficiary '${name}' already exists` },
        { status: 400 }
      );
    }

    const result = await createCredential(state.crs_hex, name);
    addBeneficiary(name, result.credential);
    setPhase(1);

    return NextResponse.json({
      name,
      phi: result.credential.phi,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
