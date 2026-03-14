// Wrapper for the veiled-core Rust CLI binary

import { execFileSync } from "child_process";
import path from "path";

const HELPER_BIN =
  process.env.HELPER_BIN ||
  path.resolve(process.cwd(), "..", "target", "release", "veiled-core");

interface HelperResult {
  [key: string]: any;
}

function callHelper(command: string, params: Record<string, any>): HelperResult {
  const input = JSON.stringify({ command, ...params });
  try {
    const output = execFileSync(HELPER_BIN, [], {
      input,
      encoding: "utf-8",
      timeout: 60000,
    });
    return JSON.parse(output.trim());
  } catch (err: any) {
    // Try to parse stderr/stdout for structured error
    const out = err.stdout?.toString().trim() || err.stderr?.toString().trim();
    if (out) {
      try {
        const parsed = JSON.parse(out);
        throw new Error(parsed.error || out);
      } catch {
        throw new Error(out);
      }
    }
    throw new Error(`veiled-core failed: ${err.message}`);
  }
}

export function createCredential(
  crsHex: string,
  name: string
): {
  credential: {
    phi: string;
    sk: string;
    r: string;
    k: string;
    friendly_name: string;
  };
} {
  return callHelper("create-credential", {
    crs_hex: crsHex,
    name,
  }) as any;
}

export function registerLocally(
  credential: { phi: string; sk: string; r: string; k: string; friendly_name: string },
  commitmentsHex: string[]
): { index: number } {
  return callHelper("register-locally", {
    credential,
    commitments_hex: commitmentsHex,
  }) as any;
}

export function createPaymentId(params: {
  credential: { phi: string; sk: string; r: string; k: string; friendly_name: string };
  crsHex: string;
  commitmentsHex: string[];
  index: number;
  setId: number;
  merchantId: number;
}): {
  pseudonym: string;
  nullifier: string;
  proof_hex: string;
  friendly_name: string;
  service_index: number;
  set_id: number;
} {
  return callHelper("create-payment-id", {
    credential: params.credential,
    crs_hex: params.crsHex,
    commitments_hex: params.commitmentsHex,
    index: params.index,
    set_id: params.setId,
    merchant_id: params.merchantId,
  }) as any;
}

export function createPaymentRequest(params: {
  credentialRHex: string;
  merchantName: string;
  crsGHex: string;
  amount: number;
}): {
  pseudonym: string;
  proof_r: string;
  proof_s: string;
  amount: number;
} {
  return callHelper("create-payment-request", {
    credential_r_hex: params.credentialRHex,
    merchant_name: params.merchantName,
    crs_g_hex: params.crsGHex,
    amount: params.amount,
  }) as any;
}
