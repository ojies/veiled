// POST /api/setup/init — Lazy, idempotent set creation from registered merchants
// Called by the beneficiary page on mount. Safe to call multiple times.

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import {
  setMerchants,
  setCrs,
  setAnonymitySet,
  setPhase,
  getState,
  setRegistryAddress,
} from "@/lib/state";
import { createWallet, walletExists } from "@/lib/wallet";

const SET_CAPACITY = 8;

export async function POST() {
  try {
    const state = getState();
    const registry = getRegistryClient();

    // Ensure registry wallet exists in state
    if (!state.registry_address) {
      if (walletExists("registry")) {
        const w = createWallet("registry"); // idempotent
        setRegistryAddress(w.address);
      }
    }

    // If already initialized, return current state
    if (state.phase >= 0 && state.crs_hex) {
      return NextResponse.json({
        merchants: state.merchants,
        crs_hex: state.crs_hex,
        set_id: state.set_id,
        capacity: state.anonymity_set?.capacity || SET_CAPACITY,
        registry_address: state.registry_address,
        already_initialized: true,
      });
    }

    // Check if merchants are registered
    const merchantsResp: any = await grpcCall(registry, "GetMerchants", {});
    const merchants = (merchantsResp.merchants || []).map((m: any) => ({
      name: m.name,
      origin: m.origin,
      credential_generator: Buffer.from(m.credential_generator).toString(
        "hex"
      ),
    }));

    if (merchants.length === 0) {
      return NextResponse.json({
        error: "No merchants registered yet. Create merchants first.",
        waiting: true,
      });
    }

    setMerchants(merchants);

    // Create anonymity set with all registered merchants
    await grpcCall(registry, "CreateSet", {
      set_id: state.set_id,
      merchant_names: merchants.map((m: any) => m.name),
      beneficiary_capacity: SET_CAPACITY,
    });

    // Fetch CRS
    const crsResp: any = await grpcCall(registry, "GetCrs", {
      set_id: state.set_id,
    });
    const crsHex = Buffer.from(crsResp.crs_bytes).toString("hex");
    setCrs(crsHex);

    setAnonymitySet({
      commitments: [],
      finalized: false,
      count: 0,
      capacity: SET_CAPACITY,
    });
    setPhase(0);

    return NextResponse.json({
      merchants,
      crs_hex: crsHex,
      set_id: state.set_id,
      capacity: SET_CAPACITY,
      registry_address: state.registry_address,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
