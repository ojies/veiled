// POST /api/beneficiary/finalize — Finalize the anonymity set
// The registry self-funds the commitment tx using collected beneficiary fees.

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, setAnonymitySet, setPhase, setSetIdBytes } from "@/lib/state";
import { faucet, createWallet } from "@/lib/wallet";
import { log, logError } from "@/lib/log";

export async function POST() {
  try {
    const registry = getRegistryClient();

    // Finalize — the registry self-funds, signs, and broadcasts the commitment tx
    log("ben/finalize", `finalizing set`);
    const finalizeResp: any = await grpcCall(registry, "FinalizeSet", {
      set_id: Buffer.alloc(32),
    });

    // The response contains the real 32-byte commitment txid as the canonical set_id
    const setIdBytes: Buffer = Buffer.from(finalizeResp.set_id);
    const setIdHex = setIdBytes.toString("hex");
    setSetIdBytes(setIdHex);
    log("ben/finalize", `FinalizeSet OK: set_id=${setIdHex.slice(0, 16)}...`);

    // Mine a block to confirm the broadcast commitment transaction
    const confirmMiner = await createWallet("faucet-miner");
    await faucet(confirmMiner.address, 1);
    log("ben/finalize", `mined 1 confirmation block`);

    // Fetch updated anonymity set using the real set_id bytes
    const updatedSet: any = await grpcCall(registry, "GetAnonymitySet", {
      set_id: setIdBytes,
    });
    const commitments = (updatedSet.commitments || []).map((c: Buffer) =>
      Buffer.from(c).toString("hex")
    );
    log("ben/finalize", `set: ${updatedSet.count} members, ${commitments.length} commitments (padded), finalized=${updatedSet.finalized}`);

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
      set_id_hex: setIdHex,
    });
  } catch (err: any) {
    const msg = err.message || String(err);
    if (msg.includes("already")) {
      log("ben/finalize", `set already finalized`);
    } else if (msg.includes("ECONNREFUSED") || msg.includes("UNAVAILABLE")) {
      logError("ben/finalize", `registry connection failed`, err);
    } else {
      logError("ben/finalize", "failed", err);
    }
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
