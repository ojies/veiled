// POST /api/merchant/receive-registration — Merchant accepts a pasted PaymentIdentityRegistration token
// Body: { merchant_name: string, registration_token: string }
// Returns: { pseudonym: string }

import { NextResponse } from "next/server";
import { receivePaymentId } from "@/lib/core";
import { getState, addMerchantIdentity } from "@/lib/state";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { merchant_name, registration_token } = await request.json();
    if (!merchant_name || !registration_token) {
      return NextResponse.json(
        { error: "merchant_name and registration_token required" },
        { status: 400 }
      );
    }

    const state = getState();

    if (!state.crs_hex) {
      return NextResponse.json({ error: "CRS not available" }, { status: 400 });
    }
    if (!state.anonymity_set?.finalized) {
      return NextResponse.json({ error: "set not finalized" }, { status: 400 });
    }

    const proc = state.merchant_processes[merchant_name];
    if (!proc) {
      return NextResponse.json(
        { error: `unknown merchant '${merchant_name}'` },
        { status: 400 }
      );
    }

    // Decode the base64 registration token
    let tokenPayload: {
      pseudonym: string;
      nullifier: string;
      set_id: number;
      service_index: number;
      friendly_name: string;
      proof_hex: string;
    };
    try {
      tokenPayload = JSON.parse(Buffer.from(registration_token, "base64").toString("utf8"));
    } catch {
      return NextResponse.json({ error: "invalid registration_token" }, { status: 400 });
    }

    log("receive-registration", `merchant='${merchant_name}', friendly_name='${tokenPayload.friendly_name}', service_index=${tokenPayload.service_index}`);

    const result = await receivePaymentId({
      crsHex: state.crs_hex,
      commitmentsHex: state.anonymity_set.commitments,
      pseudonymHex: tokenPayload.pseudonym,
      nullifierHex: tokenPayload.nullifier,
      setId: tokenPayload.set_id,
      serviceIndex: tokenPayload.service_index,
      friendlyName: tokenPayload.friendly_name,
      proofHex: tokenPayload.proof_hex,
      merchantName: proc.name,
      merchantOrigin: proc.origin,
    });

    log("receive-registration", `accepted '${tokenPayload.friendly_name}' → pseudonym=${result.pseudonym.slice(0, 16)}...`);

    addMerchantIdentity(merchant_name, {
      friendly_name: tokenPayload.friendly_name,
      pseudonym: result.pseudonym,
      nullifier: tokenPayload.nullifier,
    });

    return NextResponse.json({
      pseudonym: result.pseudonym,
      friendly_name: tokenPayload.friendly_name,
    });
  } catch (err: any) {
    logError("receive-registration", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
