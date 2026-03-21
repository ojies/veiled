// POST /api/merchant/create — Register merchant with registry, defer gRPC server spawn

import { NextResponse } from "next/server";
import { getState, addMerchantProcess } from "@/lib/state";
import { createWallet, getBalance, send, faucet, getTx } from "@/lib/wallet";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { MERCHANT_START_PORT } from "@/lib/config";
import { log, logError } from "@/lib/log";

// Track the next available port
let nextPort = MERCHANT_START_PORT;

export async function POST(request: Request) {
  try {
    const { name, origin } = await request.json();
    log("merchant/create", `name='${name}', origin='${origin}'`);
    if (!name || !origin) {
      return NextResponse.json(
        { error: "name and origin required" },
        { status: 400 }
      );
    }

    const state = getState();

    // Check if merchant already exists
    if (state.merchant_processes[name]) {
      const existing = state.merchant_processes[name];
      return NextResponse.json({
        name: existing.name,
        port: existing.port,
        status: existing.status,
        existing: true,
      });
    }

    // Fetch merchant fee and registry address
    const registry = getRegistryClient();
    const [feesResp, addrResp]: any[] = await Promise.all([
      grpcCall(registry, "GetFees", {}),
      grpcCall(registry, "GetRegistryAddress", { set_id: 0 }),
    ]);
    const merchantFee = feesResp.merchant_fee;
    const registryAddress = addrResp.address;

    // Create wallet for this merchant
    const walletName = `merchant-${name.toLowerCase().replace(/\s+/g, "-")}`;
    const wallet = await createWallet(walletName);

    // Confirmation miner (for mining blocks after sends)
    const dummyWallet = await createWallet("faucet-miner");

    // Pay merchant registration fee to registry (wallet must be funded by the user first)
    log("merchant/create", `paying ${merchantFee} sats to registry ${registryAddress.slice(0, 20)}...`);
    const sendResult = await send(walletName, registryAddress, merchantFee);
    const fundingTxid = sendResult.txid;
    log("merchant/create", `fee tx: ${fundingTxid}`);
    await faucet(dummyWallet.address, 1);

    // Find the correct vout (the output paying the registry address)
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
      return NextResponse.json(
        { error: `no output paying registry address in tx ${fundingTxid}` },
        { status: 400 }
      );
    }

    // Store spawn params — the merchant binary will be spawned by setup/init
    // after the anonymity set is created (the binary needs GetCrs which
    // requires a set to exist).
    const port = nextPort++;
    addMerchantProcess(name, {
      name,
      origin,
      port,
      pid: 0,
      status: "pending",
      spawnParams: {
        fundingTxid,
        fundingVout,
      },
    });
    log("merchant/create", `registered '${name}', port=${port}, spawn deferred until set is created`);

    // The merchant is registered with the registry via the fee payment.
    // The gRPC server process will be spawned by setup/init once the set is created.
    const balance = await getBalance(walletName);
    log("merchant/create", `'${name}' registered OK, port=${port}, balance=${balance.total}, server pending`);

    return NextResponse.json({
      name,
      port,
      address: wallet.address,
      wallet_name: walletName,
      balance: balance.total,
      status: "pending",
    });
  } catch (err: any) {
    logError("merchant/create", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
