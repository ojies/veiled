// POST /api/merchant/verify-payment — Phase 5: merchant verifies payment token

import { NextResponse } from "next/server";
import { verifyPaymentRequest } from "@/lib/core";
import { getState, addMerchantPayment } from "@/lib/state";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { merchant_name, payment_token } = await request.json();

    if (!merchant_name || !payment_token) {
      return NextResponse.json(
        { error: "merchant_name and payment_token required" },
        { status: 400 }
      );
    }

    const state = getState();

    if (!state.crs_hex) {
      return NextResponse.json({ error: "CRS not initialized" }, { status: 400 });
    }

    // Decode base64 token
    let tokenObj: { pseudonym: string; proof_r: string; proof_s: string; amount: number; merchant_name: string; friendly_name?: string };
    try {
      tokenObj = JSON.parse(Buffer.from(payment_token, "base64").toString());
    } catch {
      return NextResponse.json({ error: "invalid token format" }, { status: 400 });
    }

    const { pseudonym, proof_r, proof_s, amount, friendly_name: tokenFriendlyName } = tokenObj;

    // Extract CRS g generator
    const gHex = Buffer.from(state.crs_hex, "hex").subarray(12, 45).toString("hex");

    log("verify-payment", `merchant='${merchant_name}', pseudonym=${pseudonym.slice(0, 16)}..., amount=${amount}`);

    // Verify Schnorr proof via veiled-core
    const result = await verifyPaymentRequest({
      crsGHex: gHex,
      pseudonymHex: pseudonym,
      proofRHex: proof_r,
      proofSHex: proof_s,
    });

    if (!result.valid) {
      return NextResponse.json({ error: "invalid Schnorr proof" }, { status: 400 });
    }

    // Resolve friendly name: prefer token field, fall back to registered identities lookup
    const proc = state.merchant_processes[merchant_name];
    const identity = proc?.registered_identities?.find((i) => i.pseudonym === pseudonym);
    const friendly_name = tokenFriendlyName ?? identity?.friendly_name ?? "Unknown";

    // Store payment in merchant state
    addMerchantPayment(merchant_name, {
      beneficiary: friendly_name,
      pseudonym,
      amount,
      address: result.address,
    });

    log("verify-payment", `OK: ${merchant_name}: ${friendly_name} -> ${amount} sats -> ${result.address}`);

    return NextResponse.json({
      valid: true,
      address: result.address,
      amount,
      friendly_name,
    });
  } catch (err: any) {
    logError("verify-payment", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
