// POST /api/beneficiary/finalize — Finalize the anonymity set with real funding UTXO

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, setAnonymitySet, setPhase, setFunding } from "@/lib/state";
import { getBalance, send, faucet, createWallet } from "@/lib/wallet";
export async function POST(request: Request) {
  try {
    const state = getState();
    const registry = getRegistryClient();
    const body = await request.json().catch(() => ({}));

    // Use registry wallet to fund the VTxO tree
    // The registry has collected fees from merchants + beneficiaries
    const registryBalance = getBalance("registry");

    // Fetch fees + anonymity set in parallel
    const [feesResp, setResp]: any[] = await Promise.all([
      grpcCall(registry, "GetFees", {}),
      grpcCall(registry, "GetAnonymitySet", { set_id: state.set_id }),
    ]);
    const count = setResp.count || 1;
    const satsPerUser = Math.floor(registryBalance.confirmed / count) || feesResp.beneficiary_fee;

    // Send funding to the aggregate key address (the address the root_tx spends from)
    const fundingAmount = satsPerUser * count;

    let fundingTxid: string;
    let fundingVout = 0;

    if (body.funding_txid) {
      fundingTxid = body.funding_txid;
      fundingVout = body.funding_vout || 0;
    } else {
      // Get the aggregate address (derived from all beneficiary pubkeys)
      const aggResp: any = await grpcCall(registry, "GetAggregateAddress", {
        set_id: state.set_id,
      });
      const aggregateAddress = aggResp.address;

      // Send funding from registry wallet to the aggregate address
      const result = send("registry", aggregateAddress, fundingAmount);
      fundingTxid = result.txid;

      // Mine a block to confirm the funding UTXO
      const miner = createWallet("faucet-miner");
      faucet(miner.address, 1);
    }

    // Finalize — the registry signs and broadcasts root_tx + fanout_tx
    const txidBytes = Buffer.from(fundingTxid, "hex");
    const finalizeResp: any = await grpcCall(registry, "FinalizeSet", {
      set_id: state.set_id,
      sats_per_user: satsPerUser,
      funding_txid: txidBytes,
      funding_vout: fundingVout,
    });

    // Mine a block to confirm the broadcast commitment transactions
    const confirmMiner = createWallet("faucet-miner");
    faucet(confirmMiner.address, 1);

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
        root_txid: finalizeResp.root_txid || "",
        fanout_txid: finalizeResp.fanout_txid || "",
      },
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
