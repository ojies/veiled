// POST /api/beneficiary/finalize — Finalize the anonymity set with real funding UTXO

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, setAnonymitySet, setPhase, setFunding } from "@/lib/state";
import { getBalance, send, faucet, createWallet } from "@/lib/wallet";
import { DEFAULT_SATS_PER_USER } from "@/lib/config";

export async function POST(request: Request) {
  try {
    const state = getState();
    const registry = getRegistryClient();
    const body = await request.json().catch(() => ({}));

    // Use registry wallet to fund the VTxO tree
    // The registry has collected fees from merchants + beneficiaries
    const registryBalance = getBalance("registry");

    // Calculate sats per user from registry balance
    const setResp: any = await grpcCall(registry, "GetAnonymitySet", {
      set_id: state.set_id,
    });
    const count = setResp.count || 1;
    const satsPerUser = Math.floor(registryBalance.confirmed / count) || DEFAULT_SATS_PER_USER;

    // Send the funding amount to registry's own address (creates UTXO)
    // In a real system this would go to the VTxO tree address
    const fundingAmount = satsPerUser * count;

    let fundingTxid: string;
    let fundingVout = 0;

    if (body.funding_txid) {
      // Use provided funding txid
      fundingTxid = body.funding_txid;
      fundingVout = body.funding_vout || 0;
    } else {
      // Create a self-send to generate a UTXO
      const result = send("registry", state.registry_address || "", fundingAmount);
      fundingTxid = result.txid;
      // Mine a block to confirm
      const miner = createWallet("faucet-miner");
      faucet(miner.address, 1);
    }

    // Finalize the set with the real funding UTXO
    const txidBytes = Buffer.from(fundingTxid, "hex");
    await grpcCall(registry, "FinalizeSet", {
      set_id: state.set_id,
      sats_per_user: satsPerUser,
      funding_txid: txidBytes,
      funding_vout: fundingVout,
    });

    setFunding(fundingTxid, fundingVout, fundingAmount);

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

    // Fetch VTxO tree info
    const vtxoResp: any = await grpcCall(registry, "GetVtxoTree", {
      set_id: state.set_id,
    });

    setPhase(2);

    return NextResponse.json({
      commitments,
      finalized: true,
      count: updatedSet.count,
      capacity: updatedSet.capacity,
      funding: { txid: fundingTxid, vout: fundingVout, amount: fundingAmount },
      vtxo: {
        root_tx_size: vtxoResp.root_tx?.length || 0,
        fanout_tx_size: vtxoResp.fanout_tx?.length || 0,
      },
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
