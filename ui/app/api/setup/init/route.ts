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
import { BENEFICIARY_CAPACITY, MIN_MERCHANTS } from "@/lib/config";
import { log, logError } from "@/lib/log";

export async function POST() {
  try {
    log("setup/init", "called");
    const state = getState();
    const registry = getRegistryClient();

    // Ensure registry wallet exists in state
    if (!state.registry_address) {
      if (walletExists("registry")) {
        const w = await createWallet("registry"); // idempotent
        setRegistryAddress(w.address);
      }
    }

    // Fetch fees from registry (source of truth)
    const feesResp: any = await grpcCall(registry, "GetFees", {});
    const fees = {
      beneficiary: feesResp.beneficiary_fee || 0,
      merchant: feesResp.merchant_fee || 0,
    };

    // If already initialized, return current state
    if (state.phase >= 0 && state.crs_hex) {
      return NextResponse.json({
        merchants: state.merchants,
        crs_hex: state.crs_hex,
        set_id: state.set_id,
        capacity: state.anonymity_set?.capacity || BENEFICIARY_CAPACITY,
        registry_address: state.registry_address,
        fees,
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

    if (merchants.length < MIN_MERCHANTS) {
      log("setup/init", `waiting: ${merchants.length}/${MIN_MERCHANTS} merchants`);
      return NextResponse.json({
        error: `Need at least ${MIN_MERCHANTS} merchant(s) registered. Currently: ${merchants.length}.`,
        waiting: true,
      });
    }

    log("setup/init", `${merchants.length} merchants found: ${merchants.map((m: any) => m.name).join(", ")}`);
    setMerchants(merchants);

    // Create anonymity set with all registered merchants (idempotent — ignore "already exists")
    try {
      await grpcCall(registry, "CreateSet", {
        set_id: state.set_id,
        merchant_names: merchants.map((m: any) => m.name),
        beneficiary_capacity: BENEFICIARY_CAPACITY,
        sats_per_user: fees.beneficiary,
      });
    } catch (e: any) {
      // If the set already exists (e.g., server restarted), proceed to fetch CRS
      if (!e.message?.includes("already exists") && !e.message?.includes("ALREADY_EXISTS")) {
        throw e;
      }
    }

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
      capacity: BENEFICIARY_CAPACITY,
    });
    setPhase(0);
    log("setup/init", `initialized: set_id=${state.set_id}, CRS=${crsHex.length} hex chars, capacity=${BENEFICIARY_CAPACITY}`);

    return NextResponse.json({
      merchants,
      crs_hex: crsHex,
      set_id: state.set_id,
      capacity: BENEFICIARY_CAPACITY,
      registry_address: state.registry_address,
      fees,
    });
  } catch (err: any) {
    logError("setup/init", "failed", err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
