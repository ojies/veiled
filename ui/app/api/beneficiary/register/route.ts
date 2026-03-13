// POST /api/beneficiary/register — Phase 2: pay registration fee + register Φ with registry

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, getBeneficiary, updateBeneficiary, setAnonymitySet, setPhase } from "@/lib/state";
import { send, faucet, createWallet, getTx } from "@/lib/wallet";
import { BENEFICIARY_REGISTRATION_FEE } from "@/lib/config";

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

    // Step 1: Query the registry's payment address for this set
    const addrResp: any = await grpcCall(registry, "GetRegistryAddress", {
      set_id: state.set_id,
    });
    const registryAddress = addrResp.address;

    // Step 2: Send payment from beneficiary's wallet to registry address
    const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;
    const sendResult = send(walletName, registryAddress, BENEFICIARY_REGISTRATION_FEE);
    const fundingTxid = sendResult.txid;

    // Step 3: Mine a block to confirm (regtest only)
    const miner = createWallet("faucet-miner");
    faucet(miner.address, 1);

    // Step 4: Find the correct vout (the output paying the registry address)
    const txInfo = getTx(fundingTxid);
    let fundingVout = 0;
    if (txInfo.vout) {
      for (let i = 0; i < txInfo.vout.length; i++) {
        const output = txInfo.vout[i];
        if (output.scriptPubKey?.address === registryAddress) {
          fundingVout = i;
          break;
        }
      }
    }

    // Step 5: Register with outpoint (registry verifies payment on-chain)
    const txidBytes = Buffer.from(fundingTxid, "hex");
    const resp: any = await grpcCall(registry, "RegisterBeneficiary", {
      set_id: state.set_id,
      phi,
      name: ben.credential.friendly_name,
      email: "",
      phone: "",
      funding_txid: txidBytes,
      funding_vout: fundingVout,
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
      payment: {
        txid: fundingTxid,
        vout: fundingVout,
        amount: BENEFICIARY_REGISTRATION_FEE,
        registry_address: registryAddress,
      },
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
