// POST /api/beneficiary/payment-id — Phase 3-4: register payment identity with merchant

import { NextResponse } from "next/server";
import { getMerchantClient, grpcCall } from "@/lib/grpc";
import { createPaymentId } from "@/lib/helper";
import { getState, getBeneficiary, updateBeneficiary, setPhase } from "@/lib/state";

// Merchant gRPC addresses (must match what scripts/dev.sh starts)
const MERCHANT_PORTS: Record<string, string> = {
  CoffeeCo: "[::1]:50061",
  BookStore: "[::1]:50062",
  TechMart: "[::1]:50063",
};

export async function POST(request: Request) {
  try {
    const { beneficiary, merchant } = await request.json();
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

    // Find merchant index (1-indexed)
    const merchantIdx = state.merchants.findIndex((m) => m.name === merchant) + 1;
    if (merchantIdx === 0) {
      return NextResponse.json(
        { error: `unknown merchant '${merchant}'` },
        { status: 400 }
      );
    }

    // Generate ZK proof via helper
    const proofResult = createPaymentId({
      credential: ben.credential,
      crsHex: state.crs_hex!,
      commitmentsHex: state.anonymity_set.commitments,
      index: ben.index,
      setId: state.set_id,
      merchantId: merchantIdx,
    });

    // Submit to merchant gRPC server
    const merchantAddr = MERCHANT_PORTS[merchant];
    if (!merchantAddr) {
      return NextResponse.json(
        { error: `no address for merchant '${merchant}'` },
        { status: 400 }
      );
    }
    const merchantClient = getMerchantClient(merchantAddr);
    await grpcCall(merchantClient, "SubmitPaymentRegistration", {
      pseudonym: Buffer.from(proofResult.pseudonym, "hex"),
      public_nullifier: Buffer.from(proofResult.nullifier, "hex"),
      set_id: proofResult.set_id,
      service_index: proofResult.service_index,
      friendly_name: proofResult.friendly_name,
      proof: Buffer.from(proofResult.proof_hex, "hex"),
    });

    // Update state
    const reg = {
      merchant_name: merchant,
      pseudonym: proofResult.pseudonym,
      nullifier: proofResult.nullifier,
      status: "verified" as const,
    };
    updateBeneficiary(beneficiary, {
      registrations: [...ben.registrations, reg],
    });
    setPhase(3);

    return NextResponse.json({
      beneficiary,
      merchant,
      pseudonym: proofResult.pseudonym,
      nullifier: proofResult.nullifier,
      status: "verified",
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
