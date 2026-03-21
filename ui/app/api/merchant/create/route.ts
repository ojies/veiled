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
import { log, logError } from "@/lib/log";

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

    // Fund merchant wallet via faucet (mine 1 block to its address + blocks to mature)
    await faucet(wallet.address, 1);
    const dummyWallet = await createWallet("faucet-miner");
    await faucet(dummyWallet.address, MATURITY_BLOCKS);

    // Pay merchant registration fee to registry
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

    // Find available port
    const port = nextPort++;
    const listenAddr = `0.0.0.0:${port}`;

    // Spawn merchant gRPC server
    log("merchant/create", `spawning merchant binary on port ${port}`, { fundingTxid, fundingVout, setId: state.set_id });
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

    let stderrBuf = "";
    child.stderr?.on("data", (data: Buffer) => {
      const msg = data.toString();
      stderrBuf += msg;
      log("merchant/create", `[${name} stderr] ${msg.trim()}`);
      if (msg.includes("listening") || msg.includes("registered")) {
        const proc = state.merchant_processes[name];
        if (proc) proc.status = "running";
      }
    });

    child.on("exit", (code) => {
      log("merchant/create", `[${name}] process exited with code ${code}`);
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
          { error: `Merchant process exited before completing registration: ${stderrBuf.trim() || "(no output)"}` },
          { status: 500 }
        );
      }
    }

    if (!confirmed) {
      logError("merchant/create", `registration timed out for '${name}'`);
      return NextResponse.json(
        { error: "Merchant registration timed out — check registry logs" },
        { status: 500 }
      );
    }

    const balance = await getBalance(walletName);
    log("merchant/create", `'${name}' registered OK on port ${port}, balance=${balance.total}`);

    return NextResponse.json({
      name,
      port,
      address: wallet.address,
      wallet_name: walletName,
      balance: balance.total,
      status: "running",
    });
  } catch (err: any) {
    logError("merchant/create", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
