// Spawn a merchant gRPC server binary as a child process.

import { spawn } from "child_process";
import path from "path";
import { getState } from "./state";
import { log } from "./log";

const MERCHANT_BIN =
  process.env.MERCHANT_BIN ||
  path.resolve(process.cwd(), "..", "target", "release", "merchant");

const REGISTRY_SERVER =
  process.env.REGISTRY_SERVER || "http://[::1]:50051";

export function spawnMerchant(name: string): boolean {
  const state = getState();
  const proc = state.merchant_processes[name];
  if (!proc || !proc.spawnParams) {
    log("merchant-spawn", `no spawn params for '${name}'`);
    return false;
  }
  if (proc.status === "running" || proc.status === "starting") {
    log("merchant-spawn", `'${name}' already ${proc.status}`);
    return true;
  }

  const { fundingTxid, fundingVout } = proc.spawnParams;
  const listenAddr = `0.0.0.0:${proc.port}`;

  log("merchant-spawn", `spawning '${name}' on port ${proc.port} (set=${state.set_id})`);

  const child = spawn(
    MERCHANT_BIN,
    [
      "--name", name,
      "--origin", proc.origin,
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

  proc.pid = child.pid || 0;
  proc.status = "starting";

  child.stdout?.on("data", () => {
    if (proc.status === "starting") proc.status = "running";
  });

  child.stderr?.on("data", (data: Buffer) => {
    const msg = data.toString().trim();
    log("merchant-spawn", `[${name}] ${msg}`);
    if (msg.includes("listening") || msg.includes("registered")) {
      proc.status = "running";
    }
  });

  child.on("exit", (code) => {
    log("merchant-spawn", `[${name}] exited with code ${code}`);
    proc.status = "stopped";
  });

  return true;
}

/** Spawn all pending merchant processes. Called after the set is created. */
export function spawnPendingMerchants(): number {
  const state = getState();
  let spawned = 0;
  for (const [name, proc] of Object.entries(state.merchant_processes)) {
    if (proc.status === "pending" || proc.status === "stopped") {
      if (spawnMerchant(name)) spawned++;
    }
  }
  if (spawned > 0) {
    log("merchant-spawn", `spawned ${spawned} pending merchant(s)`);
  }
  return spawned;
}
