// POST /api/beneficiary/payment-id — Phase 3-4: register payment identity with merchant

import { NextResponse } from "next/server";
import { getMerchantClient, grpcCall } from "@/lib/grpc";
import { createPaymentId } from "@/lib/core";
import { getState, getBeneficiary, updateBeneficiary, setPhase } from "@/lib/state";
import { log, logError } from "@/lib/log";

function getMerchantAddr(name: string): string | null {
  const state = getState();
  const proc = state.merchant_processes[name];
  // Use 127.0.0.1 (not localhost) to avoid IPv6 ::1 resolution in Docker
  if (proc) return `127.0.0.1:${proc.port}`;
  return null;
}

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

    // Find merchant index (1-indexed for the protocol)
    const rawIdx = state.merchants.findIndex((m) => m.name === merchant);
    if (rawIdx === -1) {
      return NextResponse.json(
        { error: `unknown merchant '${merchant}'` },
        { status: 400 }
      );
    }
    const merchantIdx = rawIdx + 1;

    // Generate ZK proof via helper
    const proofResult = await createPaymentId({
      credential: ben.credential,
      crsHex: state.crs_hex!,
      commitmentsHex: state.anonymity_set.commitments,
      index: ben.index,
      setId: state.set_id,
      merchantId: merchantIdx,
    });

    // Submit to merchant gRPC server
    const merchantAddr = getMerchantAddr(merchant);
    log("payment-id", `connecting to merchant '${merchant}' at ${merchantAddr}`);
    if (!merchantAddr) {
      logError("payment-id", `no running merchant server for '${merchant}'`);
      return NextResponse.json(
        { error: `no running merchant server for '${merchant}'` },
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
    const msg = err.message || String(err);
    if (msg.includes("ECONNREFUSED") || msg.includes("UNAVAILABLE")) {
      logError("payment-id", `merchant gRPC connection failed — is the merchant process running?`, err);
    } else {
      logError("payment-id", "failed", err);
    }
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
