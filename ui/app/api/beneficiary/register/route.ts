// POST /api/beneficiary/register — Phase 2: register Φ with registry

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, getBeneficiary, updateBeneficiary, setAnonymitySet, setPhase } from "@/lib/state";

export async function POST(request: Request) {
  try {
    const { name } = await request.json();
    const state = getState();
    const ben = getBeneficiary(name);

    if (!ben?.credential) {
      return NextResponse.json(
        { error: `no credential for '${name}'` },
        { status: 400 }
      );
    }
    if (ben.registered) {
      return NextResponse.json(
        { error: `'${name}' already registered` },
        { status: 400 }
      );
    }

    const registry = getRegistryClient();
    const phi = Buffer.from(ben.credential.phi, "hex");

    const resp: any = await grpcCall(registry, "RegisterBeneficiary", {
      set_id: state.set_id,
      phi,
      name: ben.credential.friendly_name,
      email: "",
      phone: "",
    });

    updateBeneficiary(name, { registered: true, index: resp.index });

    // Refresh anonymity set status
    const setResp: any = await grpcCall(registry, "GetAnonymitySet", {
      set_id: state.set_id,
    });
    const commitments = (setResp.commitments || []).map((c: Buffer) =>
      Buffer.from(c).toString("hex")
    );
    setAnonymitySet({
      commitments,
      finalized: setResp.finalized,
      count: setResp.count,
      capacity: setResp.capacity,
    });
    setPhase(2);

    return NextResponse.json({
      name,
      index: resp.index,
      set_count: setResp.count,
      set_capacity: setResp.capacity,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
