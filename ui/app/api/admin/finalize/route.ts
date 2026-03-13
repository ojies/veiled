// POST /api/admin/finalize — Phase 2b: finalize the anonymity set

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, setAnonymitySet, setPhase } from "@/lib/state";

export async function POST() {
  try {
    const state = getState();
    const registry = getRegistryClient();

    // Finalize with dummy funding tx (demo mode)
    const dummyTxid = Buffer.alloc(32, 0);
    await grpcCall(registry, "FinalizeSet", {
      set_id: state.set_id,
      sats_per_user: 10000,
      funding_txid: dummyTxid,
      funding_vout: 0,
    });

    // Fetch updated anonymity set
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

    // Fetch VTxO tree info
    const vtxoResp: any = await grpcCall(registry, "GetVtxoTree", {
      set_id: state.set_id,
    });

    setPhase(2);

    return NextResponse.json({
      commitments,
      finalized: true,
      count: setResp.count,
      capacity: setResp.capacity,
      vtxo: {
        root_tx_size: vtxoResp.root_tx?.length || 0,
        fanout_tx_size: vtxoResp.fanout_tx?.length || 0,
      },
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
