// POST /api/beneficiary/payment — Phase 5: create payment request token

import { NextResponse } from "next/server";
import { createPaymentRequest, pseudonymToAddress } from "@/lib/core";
import { getState, getBeneficiary, updateBeneficiary, setPhase } from "@/lib/state";
import { log, logError } from "@/lib/log";

export async function POST(request: Request) {
  try {
    const { beneficiary, merchant, amount } = await request.json();
    const state = getState();
    const ben = getBeneficiary(beneficiary);

    if (!ben?.credential) {
      return NextResponse.json(
        { error: `no credential for '${beneficiary}'` },
        { status: 400 }
      );
    }

    if (!state.crs_hex) {
      return NextResponse.json({ error: "CRS not initialized" }, { status: 400 });
    }

    // Extract CRS g generator (first 33 bytes after header: 4+4+4 = 12 bytes offset)
    const crsBytes = Buffer.from(state.crs_hex, "hex");
    const gHex = crsBytes.subarray(12, 45).toString("hex");

    log("payment", `creating payment request: beneficiary='${beneficiary}', merchant='${merchant}', amount=${amount}`);

    // Generate Schnorr proof via veiled-core
    const proofResult = await createPaymentRequest({
      credentialRHex: ben.credential.r,
      merchantName: merchant,
      crsGHex: gHex,
      amount,
    });

    // Derive P2TR address from pseudonym
    const { address } = await pseudonymToAddress({ pseudonymHex: proofResult.pseudonym });

    // Build base64 payment token
    const tokenObj = {
      pseudonym: proofResult.pseudonym,
      proof_r: proofResult.proof_r,
      proof_s: proofResult.proof_s,
      amount,
      merchant_name: merchant,
      friendly_name: ben.credential.friendly_name,
    };
    const token = Buffer.from(JSON.stringify(tokenObj)).toString("base64");

    log("payment", `token created: address=${address}, pseudonym=${proofResult.pseudonym.slice(0, 16)}...`);

    // Store payment in beneficiary state
    updateBeneficiary(beneficiary, {
      payments: [
        ...ben.payments,
        {
          merchant_name: merchant,
          amount,
          address,
          friendly_name: ben.credential.friendly_name,
          token,
        },
      ],
    });
    setPhase(5);

    return NextResponse.json({
      token,
      address,
      pseudonym: proofResult.pseudonym,
      amount,
      merchant,
    });
  } catch (err: any) {
    logError("payment", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
