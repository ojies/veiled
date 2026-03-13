// POST /api/merchant/create — Spawn a merchant gRPC server + register with registry

import { NextResponse } from "next/server";
import { spawn } from "child_process";
import path from "path";
import { getState, addMerchantProcess } from "@/lib/state";
import { createWallet, getBalance, send, faucet } from "@/lib/wallet";
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

    // Fetch merchant fee from registry
    const registry = getRegistryClient();
    const feesResp: any = await grpcCall(registry, "GetFees", {});
    const merchantFee = feesResp.merchant_fee;

    // Create wallet for this merchant
    const walletName = `merchant-${name.toLowerCase().replace(/\s+/g, "-")}`;
    const wallet = createWallet(walletName);

    // Fund merchant wallet via faucet (mine 1 block to its address + blocks to mature)
    faucet(wallet.address, 1);
    const dummyWallet = createWallet("faucet-miner");
    faucet(dummyWallet.address, MATURITY_BLOCKS);
    if (state.registry_address) {
      send(walletName, state.registry_address, merchantFee);
      faucet(dummyWallet.address, 1);
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

    // Give it a moment to start
    await new Promise((r) => setTimeout(r, MERCHANT_STARTUP_DELAY));

    const balance = getBalance(walletName);

    return NextResponse.json({
      name,
      port,
      address: wallet.address,
      wallet_name: walletName,
      balance: balance.total,
      status: state.merchant_processes[String(name)]?.status ?? "starting",
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
