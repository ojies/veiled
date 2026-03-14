// POST /api/beneficiary/finalize — Finalize the anonymity set
// The registry self-funds the commitment tx using collected beneficiary fees.

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, setAnonymitySet, setPhase } from "@/lib/state";
import { faucet, createWallet } from "@/lib/wallet";

export async function POST() {
  try {
    const state = getState();
    const registry = getRegistryClient();

    // Finalize — the registry self-funds, signs, and broadcasts the commitment tx
    await grpcCall(registry, "FinalizeSet", { set_id: state.set_id });

    // Mine a block to confirm the broadcast commitment transaction
    const confirmMiner = createWallet("faucet-miner");
    faucet(confirmMiner.address, 1);

    // Fetch updated anonymity set
    const updatedSet: any = await grpcCall(registry, "GetAnonymitySet", {
      set_id: state.set_id,
    });
    const commitments = (updatedSet.commitments || []).map((c: Buffer) =>
      Buffer.from(c).toString("hex")
    );

    setAnonymitySet({
      commitments,
      finalized: updatedSet.finalized,
      count: updatedSet.count,
      capacity: updatedSet.capacity,
    });

    setPhase(2);

    return NextResponse.json({
      commitments,
      finalized: true,
      count: updatedSet.count,
      capacity: updatedSet.capacity,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
