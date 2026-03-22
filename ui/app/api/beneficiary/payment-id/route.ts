// POST /api/beneficiary/payment-id — Phase 3: create payment identity registration token
// Returns a base64-encoded token the beneficiary copies and pastes into the merchant UI.

import { NextResponse } from "next/server";
import { createPaymentId } from "@/lib/core";
import { getState, getBeneficiary, updateBeneficiary, setPhase } from "@/lib/state";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { beneficiary, merchant_id } = await request.json();
    const state = getState();
    const ben = getBeneficiary(beneficiary);

    if (!ben?.credential || ben.index === null) {
      return NextResponse.json(
        { error: `'${beneficiary}' not registered` },
        { status: 400 }
      );
    }
    if (!state.anonymity_set?.finalized) {
      return NextResponse.json(
        { error: "set not finalized" },
        { status: 400 }
      );
    }
    if (!merchant_id || typeof merchant_id !== "number") {
      return NextResponse.json(
        { error: "merchant_id (number) required" },
        { status: 400 }
      );
    }

    log("payment-id", `creating registration for '${beneficiary}', merchant_id=${merchant_id}`);

    // Generate ZK proof via helper
    const proofResult = await createPaymentId({
      credential: ben.credential,
      crsHex: state.crs_hex!,
      commitmentsHex: state.anonymity_set.commitments,
      index: ben.index,
      setId: state.set_id,
      merchantId: merchant_id,
    });

    // Pack all fields into a base64 token for copy-paste to merchant UI
    const tokenPayload = {
      pseudonym: proofResult.pseudonym,
      nullifier: proofResult.nullifier,
      set_id: proofResult.set_id,
      service_index: proofResult.service_index,
      friendly_name: proofResult.friendly_name,
      proof_hex: proofResult.proof_hex,
    };
    const registration_token = Buffer.from(JSON.stringify(tokenPayload)).toString("base64");

    // Record locally as pending (pseudonym confirmed once merchant accepts)
    const reg = {
      merchant_name: `merchant_id:${merchant_id}`,
      pseudonym: proofResult.pseudonym,
      nullifier: proofResult.nullifier,
      status: "pending" as const,
    };
    updateBeneficiary(beneficiary, {
      registrations: [...ben.registrations, reg],
    });
    setPhase(3);

    log("payment-id", `token created for '${beneficiary}', pseudonym=${proofResult.pseudonym.slice(0, 16)}...`);

    return NextResponse.json({
      beneficiary,
      merchant_id,
      pseudonym: proofResult.pseudonym,
      nullifier: proofResult.nullifier,
      registration_token,
    });
  } catch (err: any) {
    logError("payment-id", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
