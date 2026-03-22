// POST /api/setup/seed-merchants — Merchant faucet: auto-create a funded, registered merchant

import { NextResponse } from "next/server";
import { spawn } from "child_process";
import path from "path";
import { getState, addMerchantProcess } from "@/lib/state";
import { createWallet, faucet, send, getTx } from "@/lib/wallet";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import {
  MERCHANT_START_PORT,
  MERCHANT_STARTUP_DELAY,
  MATURITY_BLOCKS,
} from "@/lib/config";

const MERCHANT_BIN =
  process.env.MERCHANT_BIN ||
  path.resolve(process.cwd(), "../target/release/merchant");

const REGISTRY_SERVER =
  process.env.REGISTRY_SERVER || "http://[::1]:50051";

const SEED_NAME = "SeedMerchant";
const SEED_ORIGIN = "https://seed.veiled.local";

let nextSeedPort = MERCHANT_START_PORT + 100; // offset from regular merchants

export async function POST() {
  try {
    const state = getState();

    // If seed merchant already exists, return it
    if (state.merchant_processes[SEED_NAME]) {
      const existing = state.merchant_processes[SEED_NAME];
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
      grpcCall(registry, "GetRegistryAddress", { set_id: Buffer.alloc(32) }),
    ]);
    const merchantFee = feesResp.merchant_fee;
    const registryAddress = addrResp.address;

    // Create and fund wallet
    const walletName = `merchant-${SEED_NAME.toLowerCase()}`;
    const wallet = await createWallet(walletName);

    // Confirmation miner
    const dummyWallet = await createWallet("faucet-miner");

    // Pay merchant registration fee
    const sendResult = await send(walletName, registryAddress, merchantFee);
    const fundingTxid = sendResult.txid;
    await faucet(dummyWallet.address, 1);

    // Find correct vout
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

    // Spawn merchant gRPC server
    const port = nextSeedPort++;
    const listenAddr = `0.0.0.0:${port}`;

    const child = spawn(
      MERCHANT_BIN,
      [
        "--name", SEED_NAME,
        "--origin", SEED_ORIGIN,
        "--listen", listenAddr,
        "--set-id", String(state.set_id),
        "--registry-server", REGISTRY_SERVER,
        "--funding-txid", fundingTxid,
        "--funding-vout", String(fundingVout),
      ],
      {
        stdio: ["ignore", "pipe", "pipe"],
        detached: false,
      }
    );

    const pid = child.pid || 0;

    addMerchantProcess(SEED_NAME, {
      name: SEED_NAME,
      origin: SEED_ORIGIN,
      merchant_id: 0,
      port,
      pid,
      status: "starting",
    });

    child.stdout?.on("data", () => {
      const proc = state.merchant_processes[SEED_NAME];
      if (proc && proc.status === "starting") {
        proc.status = "running";
      }
    });

    child.stderr?.on("data", (data: Buffer) => {
      const msg = data.toString();
      if (msg.includes("listening") || msg.includes("registered")) {
        const proc = state.merchant_processes[SEED_NAME];
        if (proc) proc.status = "running";
      }
    });

    child.on("exit", () => {
      const proc = state.merchant_processes[SEED_NAME];
      if (proc) proc.status = "stopped";
    });

    await new Promise((r) => setTimeout(r, MERCHANT_STARTUP_DELAY));

    return NextResponse.json({
      name: SEED_NAME,
      port,
      status: state.merchant_processes[String(SEED_NAME)]?.status ?? "starting",
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
