// Wrapper for the veiled-core Rust CLI binary.
// Uses a persistent daemon process for fast IPC.

import path from "path";
import { ProcessPool } from "./process-pool";

const CORE_BIN =
  process.env.CORE_BIN ||
  path.resolve(process.cwd(), "..", "target", "release", "veiled-core");

const pool = new ProcessPool(CORE_BIN);

async function callCore(command: string, params: Record<string, any>): Promise<any> {
  return pool.call({ command, ...params });
}

export async function createCredential(
  crsHex: string,
  name: string
): Promise<{
  credential: {
    phi: string;
    sk: string;
    r: string;
    k: string;
    friendly_name: string;
  };
}> {
  return callCore("create-credential", {
    crs_hex: crsHex,
    name,
  });
}

export async function registerLocally(
  credential: { phi: string; sk: string; r: string; k: string; friendly_name: string },
  commitmentsHex: string[]
): Promise<{ index: number }> {
  return callCore("register-locally", {
    credential,
    commitments_hex: commitmentsHex,
  });
}

export async function createPaymentId(params: {
  credential: { phi: string; sk: string; r: string; k: string; friendly_name: string };
  crsHex: string;
  commitmentsHex: string[];
  index: number;
  setId: number;
  merchantId: number;
}): Promise<{
  pseudonym: string;
  nullifier: string;
  proof_hex: string;
  friendly_name: string;
  service_index: number;
  set_id: number;
}> {
  return callCore("create-payment-id", {
    credential: params.credential,
    crs_hex: params.crsHex,
    commitments_hex: params.commitmentsHex,
    index: params.index,
    set_id: params.setId,
    merchant_id: params.merchantId,
  });
}

export async function buildCrs(params: {
  merchants: Array<{ name: string; origin: string }>;
  setSize: number;
}): Promise<{ crs_hex: string }> {
  return callCore("build-crs", {
    merchants: params.merchants,
    set_size: params.setSize,
  });
}

export async function createPaymentRequest(params: {
  credentialRHex: string;
  merchantName: string;
  crsGHex: string;
  amount: number;
}): Promise<{
  pseudonym: string;
  proof_r: string;
  proof_s: string;
  amount: number;
}> {
  return callCore("create-payment-request", {
    credential_r_hex: params.credentialRHex,
    merchant_name: params.merchantName,
    crs_g_hex: params.crsGHex,
    amount: params.amount,
  });
}
