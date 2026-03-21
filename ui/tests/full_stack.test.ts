/**
 * Full-stack integration test for the Veiled UI.
 *
 * Assumes the following are already running:
 *   - Bitcoin Core (regtest) at BITCOIN_RPC_URL
 *   - veiled-registry-grpc at REGISTRY_SERVER
 *   - Next.js dev/prod server at NEXT_URL
 *
 * Run with:
 *   npm run test:e2e
 *
 * Or manually:
 *   NEXT_URL=http://localhost:3000 npx vitest run tests/full_stack.test.ts
 */

import { describe, it, expect, beforeAll } from "vitest";

const NEXT_URL = process.env.NEXT_URL || "http://localhost:3000";
const BITCOIN_RPC_URL = process.env.BITCOIN_RPC_URL || "http://localhost:18443";
const BITCOIN_RPC_USER = process.env.BITCOIN_RPC_USER || "user";
const BITCOIN_RPC_PASS = process.env.BITCOIN_RPC_PASS || "password";

// ── helpers ──────────────────────────────────────────────────────────────────

async function api(path: string, body?: object): Promise<any> {
  const method = body !== undefined ? "POST" : "GET";
  const res = await fetch(`${NEXT_URL}${path}`, {
    method,
    headers: body !== undefined ? { "Content-Type": "application/json" } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const json = await res.json();
  if (!res.ok) {
    throw new Error(`${method} ${path} → ${res.status}: ${JSON.stringify(json)}`);
  }
  return json;
}

async function bitcoinRpc(method: string, params: any[] = []): Promise<any> {
  const res = await fetch(BITCOIN_RPC_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization:
        "Basic " + Buffer.from(`${BITCOIN_RPC_USER}:${BITCOIN_RPC_PASS}`).toString("base64"),
    },
    body: JSON.stringify({ jsonrpc: "1.0", id: "test", method, params }),
  });
  const data = await res.json();
  if (data.error) throw new Error(`bitcoind ${method}: ${JSON.stringify(data.error)}`);
  return data.result;
}

/** Mine n blocks to a throw-away address to advance the chain */
async function mineBlocks(n: number): Promise<void> {
  const addr = await bitcoinRpc("getnewaddress", []);
  await bitcoinRpc("generatetoaddress", [n, addr]);
}

// ── test ─────────────────────────────────────────────────────────────────────

describe("full-stack protocol flow", () => {
  beforeAll(async () => {
    // Reset Next.js in-memory state so each test run starts clean
    await api("/api/reset", {});
  });

  it("Phase 0: fund miner wallet and pre-mine maturity blocks", async () => {
    // Mine 200 blocks so the miner wallet (used by faucet) has coins + maturity
    await mineBlocks(200);
  });

  it("Phase 0: create and fund two merchants", async () => {
    // Fund the merchant wallets via the faucet (names mode creates wallets + sends)
    const faucetResp = await api("/api/wallet/faucet", {
      names: ["merchant-alpha", "merchant-beta"],
    });
    expect(faucetResp.results["merchant-alpha"].funded).toBe(true);
    expect(faucetResp.results["merchant-beta"].funded).toBe(true);
  });

  it("Phase 0: register two merchants with the registry", async () => {
    const alpha = await api("/api/merchant/create", {
      name: "Alpha",
      origin: "https://alpha.veiled.local",
    });
    expect(alpha.name).toBe("Alpha");
    expect(alpha.status).toBe("pending"); // spawn deferred until set created

    const beta = await api("/api/merchant/create", {
      name: "Beta",
      origin: "https://beta.veiled.local",
    });
    expect(beta.name).toBe("Beta");
    expect(beta.status).toBe("pending");
  });

  it("Phase 0: setup/init builds CRS from merchants and reaches phase 0", async () => {
    const init = await api("/api/setup/init", {});
    expect(init.crs_hex).toBeTruthy();
    expect(init.crs_hex.length).toBeGreaterThan(100);
    expect(init.merchants.length).toBeGreaterThanOrEqual(2);
    expect(init.capacity).toBeGreaterThan(0);
  });

  let beneficiaryNames: string[] = [];

  it("Phase 1: create beneficiary credentials", async () => {
    beneficiaryNames = ["Alice", "Bob"];

    for (const name of beneficiaryNames) {
      const cred = await api("/api/beneficiary/credential", { name });
      expect(cred.name).toBe(name);
      expect(cred.phi).toBeTruthy();
      expect(cred.phi.length).toBeGreaterThan(32);
    }
  });

  it("Phase 1: fund beneficiary wallets", async () => {
    const walletNames = beneficiaryNames.map(
      (n) => `beneficiary-${n.toLowerCase()}`
    );
    const faucetResp = await api("/api/wallet/faucet", { names: walletNames });
    for (const wn of walletNames) {
      expect(faucetResp.results[wn].funded).toBe(true);
    }
  });

  it("Phase 1: register beneficiaries with the registry", async () => {
    for (const name of beneficiaryNames) {
      const reg = await api("/api/beneficiary/register", { name });
      expect(reg.name).toBe(name);
      expect(typeof reg.index).toBe("number");
      expect(reg.payment.txid).toBeTruthy();
    }
  });

  it("Phase 2: finalize the anonymity set", async () => {
    const fin = await api("/api/beneficiary/finalize", {});
    expect(fin.finalized).toBe(true);
    expect(fin.set_id_hex).toBeTruthy();
    expect(fin.set_id_hex.length).toBe(64); // 32-byte hex
    expect(fin.commitments.length).toBeGreaterThan(0);
    expect(fin.count).toBe(beneficiaryNames.length);
  });

  it("Phase 2: setup/init returns cached state after finalization", async () => {
    const init = await api("/api/setup/init", {});
    expect(init.already_initialized).toBe(true);
    expect(init.crs_hex).toBeTruthy();
    expect(init.merchants.length).toBeGreaterThanOrEqual(2);
  });

  it("Phase 2: check anonymity set state is finalized", async () => {
    const state = await api("/api/state");
    expect(state.phase).toBeGreaterThanOrEqual(2);
    expect(state.set_id_bytes).toBeTruthy();
    expect(state.anonymity_set?.finalized).toBe(true);
    expect(state.anonymity_set?.count).toBe(beneficiaryNames.length);
    expect(state.anonymity_set?.commitments.length).toBeGreaterThan(0);
  });
});
