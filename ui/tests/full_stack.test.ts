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
const BITCOIN_RPC_USER = process.env.BITCOIN_RPC_USER || "veiled";
const BITCOIN_RPC_PASS = process.env.BITCOIN_RPC_PASS || "veiled";

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

async function bitcoinRpc(method: string, params: any[] = [], wallet?: string): Promise<any> {
  const url = wallet ? `${BITCOIN_RPC_URL}/wallet/${wallet}` : BITCOIN_RPC_URL;
  const res = await fetch(url, {
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

/** Mine n blocks, directing coinbase rewards to the UI faucet miner wallet */
async function mineBlocks(n: number): Promise<void> {
  // Get the veiled miner wallet's address so block rewards go there
  let minerAddr: string;
  try {
    const res = await api("/api/wallet/create", { name: "miner" });
    minerAddr = res.address;
  } catch {
    // Fallback: mine to a throwaway bitcoind address if miner wallet unavailable
    try { await bitcoinRpc("createwallet", ["test-miner"]); } catch {}
    minerAddr = await bitcoinRpc("getnewaddress", [], "test-miner");
  }
  await bitcoinRpc("generatetoaddress", [n, minerAddr]);
}

// ── test ─────────────────────────────────────────────────────────────────────

describe("full-stack protocol flow", () => {
  beforeAll(async () => {
    // Reset Next.js in-memory state so each test run starts clean
    await api("/api/reset", {});
  });

  let alphaId: number;
  let betaId: number;
  let aliceAlphaToken: string;
  let bobAlphaToken: string;
  let beneficiaryNames: string[] = [];
  let alicePaymentToken: string;
  let alicePaymentAddress: string;

  it("Phase 0: fund miner wallet and pre-mine maturity blocks", async () => {
    // Mine 300 blocks to the miner wallet:
    // - 200 of those will be mature at the end (> 100 confirmations)
    // - Provides 200 * 50 BTC = 10,000 BTC in spendable balance for the faucet
    await mineBlocks(300);
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
    expect(typeof alpha.merchant_id).toBe("number");
    alphaId = alpha.merchant_id;

    const beta = await api("/api/merchant/create", {
      name: "Beta",
      origin: "https://beta.veiled.local",
    });
    expect(beta.name).toBe("Beta");
    expect(beta.status).toBe("pending");
    expect(typeof beta.merchant_id).toBe("number");
    betaId = beta.merchant_id;
  });

  it("Phase 0: setup/init builds CRS from merchants and reaches phase 0", async () => {
    const init = await api("/api/setup/init", {});
    expect(init.crs_hex).toBeTruthy();
    expect(init.crs_hex.length).toBeGreaterThan(100);
    expect(init.merchants.length).toBeGreaterThanOrEqual(2);
    expect(init.capacity).toBeGreaterThan(0);
  });

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

  it("Phase 3: beneficiaries create payment-id tokens for merchant Alpha", async () => {
    const alice = await api("/api/beneficiary/payment-id", {
      beneficiary: "Alice",
      merchant_id: alphaId,
    });
    expect(alice.registration_token).toBeTruthy();
    expect(alice.pseudonym).toBeTruthy();
    aliceAlphaToken = alice.registration_token;

    const bob = await api("/api/beneficiary/payment-id", {
      beneficiary: "Bob",
      merchant_id: alphaId,
    });
    expect(bob.registration_token).toBeTruthy();
    expect(bob.pseudonym).toBeTruthy();
    bobAlphaToken = bob.registration_token;
  });

  it("Phase 4: merchant Alpha receives registration tokens and verifies ZK proofs", async () => {
    const aliceReg = await api("/api/merchant/receive-registration", {
      merchant_name: "Alpha",
      registration_token: aliceAlphaToken,
    });
    expect(aliceReg.pseudonym).toBeTruthy();
    expect(aliceReg.friendly_name).toBe("Alice");

    const bobReg = await api("/api/merchant/receive-registration", {
      merchant_name: "Alpha",
      registration_token: bobAlphaToken,
    });
    expect(bobReg.pseudonym).toBeTruthy();
    expect(bobReg.friendly_name).toBe("Bob");

    // Pseudonyms are merchant-specific — must be distinct
    expect(aliceReg.pseudonym).not.toBe(bobReg.pseudonym);
  });

  it("Phase 4: merchant Alpha's identity list contains both beneficiaries", async () => {
    const res = await api("/api/merchant/identities?merchant=Alpha");
    expect(res.identities.length).toBe(2);
    const names = res.identities.map((i: any) => i.beneficiary);
    expect(names).toContain("Alice");
    expect(names).toContain("Bob");
  });

  it("Phase 5: Alice creates a payment request token for merchant Alpha", async () => {
    const res = await api("/api/beneficiary/payment", {
      beneficiary: "Alice",
      merchant: "Alpha",
      amount: 5000,
    });
    expect(res.token).toBeTruthy();
    expect(res.address).toMatch(/^bcrt1p/);
    expect(res.amount).toBe(5000);
    alicePaymentToken = res.token;
    alicePaymentAddress = res.address;
  });

  it("Phase 5: merchant Alpha verifies the payment token", async () => {
    const res = await api("/api/merchant/verify-payment", {
      merchant_name: "Alpha",
      payment_token: alicePaymentToken,
    });
    expect(res.valid).toBe(true);
    expect(res.address).toBe(alicePaymentAddress);
    expect(res.amount).toBe(5000);
    expect(res.friendly_name).toBe("Alice");
  });

  it("Phase 5: merchant Alpha's payments list includes Alice's payment", async () => {
    const res = await api("/api/merchant/payments?merchant=Alpha");
    const alice = res.payments.find((p: any) => p.beneficiary === "Alice");
    expect(alice).toBeDefined();
    expect(alice.amount).toBe(5000);
    expect(alice.address).toBe(alicePaymentAddress);
  });

  it("Phase 5: merchant sends BTC and scantxoutset shows UTXO at payment address", async () => {
    await api("/api/wallet/send", {
      from: "merchant-alpha",
      to_address: alicePaymentAddress,
      amount_sats: 5000,
    });
    // Mine a confirmation block via the existing faucet mechanism (avoids miner wallet issues)
    await api("/api/wallet/faucet", { names: ["faucet-miner"] });
    const scan = await api(`/api/wallet/scan-address?address=${alicePaymentAddress}`);
    expect(scan.total_amount_sats).toBeGreaterThanOrEqual(5000);
    // Beneficiary payment receipt view: utxos must include txid and BTC amount
    expect(scan.utxos.length).toBeGreaterThan(0);
    const utxo = scan.utxos[0];
    expect(utxo.txid).toBeTruthy();
    expect(utxo.txid).toHaveLength(64); // 32-byte hex
    expect(Math.round(utxo.amount * 1e8)).toBeGreaterThanOrEqual(5000);
  });
});
