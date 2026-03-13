// POST /api/beneficiary/payment — Phase 5: request payment from merchant

import { NextResponse } from "next/server";
import { getMerchantClient, grpcCall } from "@/lib/grpc";
import { createPaymentRequest } from "@/lib/helper";
import { getState, getBeneficiary, updateBeneficiary, setPhase } from "@/lib/state";

const MERCHANT_PORTS: Record<string, string> = {
  CoffeeCo: "[::1]:50061",
  BookStore: "[::1]:50062",
  TechMart: "[::1]:50063",
};

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

    // Check registration exists for this merchant
    const reg = ben.registrations.find((r) => r.merchant_name === merchant);
    if (!reg) {
      return NextResponse.json(
        { error: `'${beneficiary}' not registered with '${merchant}'` },
        { status: 400 }
      );
    }

    // Extract CRS g generator (first 33 bytes after header: 4+4+4 = 12 bytes offset)
    // CRS format: security_param(4) + L(4) + set_size(4) + g(33) + ...
    const crsBytes = Buffer.from(state.crs_hex!, "hex");
    const gHex = crsBytes.subarray(12, 12 + 33).toString("hex");

    // Generate Schnorr proof via helper
    const proofResult = createPaymentRequest({
      credentialRHex: ben.credential.r,
      merchantName: merchant,
      crsGHex: gHex,
      amount,
    });

    // Submit to merchant
    const merchantAddr = MERCHANT_PORTS[merchant];
    const merchantClient = getMerchantClient(merchantAddr);
    const resp: any = await grpcCall(merchantClient, "SubmitPaymentRequest", {
      amount,
      pseudonym: Buffer.from(proofResult.pseudonym, "hex"),
      proof_r: Buffer.from(proofResult.proof_r, "hex"),
      proof_s: Buffer.from(proofResult.proof_s, "hex"),
    });

    // Update state
    const payment = {
      merchant_name: merchant,
      amount,
      address: resp.address,
      friendly_name: resp.friendly_name,
    };
    updateBeneficiary(beneficiary, {
      payments: [...ben.payments, payment],
    });
    setPhase(5);

    return NextResponse.json({
      beneficiary,
      merchant,
      amount,
      address: resp.address,
      friendly_name: resp.friendly_name,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
