// POST /api/merchant/create — Spawn a merchant gRPC server + register with registry

import { NextResponse } from "next/server";
import { spawn } from "child_process";
import path from "path";
import { getState, addMerchantProcess } from "@/lib/state";
import { createWallet, getBalance, send, faucet, getTx } from "@/lib/wallet";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import {
  MERCHANT_START_PORT,
  MATURITY_BLOCKS,
} from "@/lib/config";

const MERCHANT_BIN =
  process.env.MERCHANT_BIN ||
  path.resolve(process.cwd(), "../target/release/merchant");

const REGISTRY_SERVER =
  process.env.REGISTRY_SERVER || "http://[::1]:50051";

// Track the next available port
let nextPort = MERCHANT_START_PORT;

export async function POST(request: Request) {
  try {
    const { name, origin } = await request.json();
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
    const wallet = createWallet(walletName);

    // Fund merchant wallet via faucet (mine 1 block to its address + blocks to mature)
    faucet(wallet.address, 1);
    const dummyWallet = createWallet("faucet-miner");
    faucet(dummyWallet.address, MATURITY_BLOCKS);

    // Pay merchant registration fee to registry
    const sendResult = send(walletName, registryAddress, merchantFee);
    const fundingTxid = sendResult.txid;
    faucet(dummyWallet.address, 1);

    // Find the correct vout (the output paying the registry address)
    const txInfo = getTx(fundingTxid);
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

    // Find available port
    const port = nextPort++;
    const listenAddr = `0.0.0.0:${port}`;

    // Spawn merchant gRPC server
    const child = spawn(
      MERCHANT_BIN,
      [
        "--name", name,
        "--origin", origin,
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

    // Track the process
    addMerchantProcess(name, {
      name,
      origin,
      port,
      pid,
      status: "starting",
    });

    // Update status when the process starts producing output
    child.stdout?.on("data", () => {
      const proc = state.merchant_processes[name];
      if (proc && proc.status === "starting") {
        proc.status = "running";
      }
    });

    child.stderr?.on("data", (data: Buffer) => {
      // Merchant logs go to stderr via tracing
      const msg = data.toString();
      if (msg.includes("listening") || msg.includes("registered")) {
        const proc = state.merchant_processes[name];
        if (proc) proc.status = "running";
      }
    });

    child.on("exit", () => {
      const proc = state.merchant_processes[name];
      if (proc) proc.status = "stopped";
    });

    // Poll the registry until the merchant appears in GetMerchants (max ~15s)
    const maxWaitMs = 15_000;
    const pollIntervalMs = 500;
    const deadline = Date.now() + maxWaitMs;
    let confirmed = false;

    while (Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, pollIntervalMs));
      try {
        const merchantsResp: any = await grpcCall(registry, "GetMerchants", {});
        const registered = (merchantsResp.merchants || []).some(
          (m: any) => m.name === name
        );
        if (registered) {
          confirmed = true;
          break;
        }
      } catch {
        // registry not ready yet, keep polling
      }

      // Also bail if the process already exited
      const proc = state.merchant_processes[name];
      if (proc?.status === "stopped") {
        return NextResponse.json(
          { error: `Merchant process exited before completing registration` },
          { status: 500 }
        );
      }
    }

    if (!confirmed) {
      return NextResponse.json(
        { error: "Merchant registration timed out — check registry logs" },
        { status: 500 }
      );
    }

    const balance = getBalance(walletName);

    return NextResponse.json({
      name,
      port,
      address: wallet.address,
      wallet_name: walletName,
      balance: balance.total,
      status: "running",
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
