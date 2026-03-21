// POST /api/beneficiary/register — Phase 2: pay registration fee + register Φ with registry

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState, getBeneficiary, updateBeneficiary, setAnonymitySet, setPhase } from "@/lib/state";
import { send, faucet, createWallet, getTx } from "@/lib/wallet";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { name } = await request.json();
    log("ben/register", `starting registration for '${name}'`);
    const state = getState();
    const ben = getBeneficiary(name);

    if (!ben?.credential) {
      logError("ben/register", `no credential for '${name}'`);
      return NextResponse.json(
        { error: `no credential for '${name}'` },
        { status: 400 }
      );
    }
    if (ben.registered) {
      log("ben/register", `'${name}' already registered`);
      return NextResponse.json(
        { error: `'${name}' already registered` },
        { status: 400 }
      );
    }

    const registry = getRegistryClient();
    const phi = Buffer.from(ben.credential.phi, "hex");

    // Step 1: Query the registry's payment address and fees
    log("ben/register", `querying registry for address and fees (set_id=${state.set_id})`);
    const [addrResp, feesResp]: any[] = await Promise.all([
      grpcCall(registry, "GetRegistryAddress", { set_id: state.set_id }),
      grpcCall(registry, "GetFees", {}),
    ]);
    const registryAddress = addrResp.address;
    const beneficiaryFee = feesResp.beneficiary_fee;
    log("ben/register", `registry address: ${registryAddress.slice(0, 20)}..., fee: ${beneficiaryFee} sats`);

    // Step 2: Send payment from beneficiary's wallet to registry address
    const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;
    log("ben/register", `sending ${beneficiaryFee} sats from '${walletName}' to registry`);
    const sendResult = await send(walletName, registryAddress, beneficiaryFee);
    const fundingTxid = sendResult.txid;
    log("ben/register", `fee tx: ${fundingTxid}`);

    // Step 3: Mine a block to confirm (regtest only)
    const miner = await createWallet("faucet-miner");
    await faucet(miner.address, 1);
    log("ben/register", `mined 1 confirmation block`);

    // Step 4: Find the correct vout (the output paying the registry address)
    const txInfo = await getTx(fundingTxid);
    let fundingVout = -1;
    if (txInfo.vout) {
      for (let i = 0; i < txInfo.vout.length; i++) {
        const output = txInfo.vout[i];
        if (output.scriptPubKey?.address === registryAddress) {
          fundingVout = i;
          break;
        }
      }
    }
    if (fundingVout === -1) {
      logError("ben/register", `vout not found for registry address in tx ${fundingTxid}`);
      return NextResponse.json(
        { error: `no output paying registry address in tx ${fundingTxid}` },
        { status: 400 }
      );
    }
    log("ben/register", `found vout=${fundingVout} in tx ${fundingTxid.slice(0, 12)}...`);

    // Step 5: Register with outpoint (registry verifies payment on-chain)
    const txidBytes = Buffer.from(fundingTxid, "hex");
    log("ben/register", `calling RegisterBeneficiary on registry (set=${state.set_id}, phi=${ben.credential.phi.slice(0, 16)}...)`);
    const resp: any = await grpcCall(registry, "RegisterBeneficiary", {
      set_id: state.set_id,
      phi,
      name: ben.credential.friendly_name,
      email: "",
      phone: "",
      funding_txid: txidBytes,
      funding_vout: fundingVout,
    });
    log("ben/register", `registered '${name}' at index ${resp.index}`);

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
    log("ben/register", `set status: ${setResp.count}/${setResp.capacity}, finalized=${setResp.finalized}`);

    return NextResponse.json({
      name,
      index: resp.index,
      set_count: setResp.count,
      set_capacity: setResp.capacity,
      payment: {
        txid: fundingTxid,
        vout: fundingVout,
        amount: beneficiaryFee,
        registry_address: registryAddress,
      },
    });
  } catch (err: any) {
    const msg = err.message || String(err);
    if (msg.includes("ECONNREFUSED") || msg.includes("UNAVAILABLE") || msg.includes("connect")) {
      logError("ben/register", `registry connection failed`, err);
    } else {
      logError("ben/register", "failed", err);
    }
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
