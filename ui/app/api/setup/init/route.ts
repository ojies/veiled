// POST /api/setup/init — Fetch system state from the registry.
// The registry auto-creates sets when enough merchants register.
// This endpoint just reads the current state and caches it locally.

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
import { buildCrs } from "@/lib/core";
import { BENEFICIARY_CAPACITY, MIN_MERCHANTS } from "@/lib/config";
import { log, logError } from "@/lib/log";
import { spawnPendingMerchants } from "@/lib/merchant-spawn";

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

    // If already initialized, spawn any pending merchants and return cached state
    if (state.phase >= 0 && state.crs_hex) {
      spawnPendingMerchants();
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
      credential_generator: Buffer.from(m.credential_generator).toString("hex"),
    }));

    if (merchants.length < MIN_MERCHANTS) {
      log("setup/init", `waiting: ${merchants.length}/${MIN_MERCHANTS} merchants`);
      return NextResponse.json({
        error: `Need at least ${MIN_MERCHANTS} merchant(s) registered. Currently: ${merchants.length}.`,
        waiting: true,
        fees,
      });
    }

    log("setup/init", `${merchants.length} merchants found: ${merchants.map((m: any) => m.name).join(", ")}`);
    setMerchants(merchants);

    // Build CRS locally from merchant generators (deterministic — no registry round-trip needed).
    // This breaks the circular dependency: beneficiaries need CRS to create credentials,
    // but the registry only has CRS after finalization. We derive it ourselves from merchant names.
    log("setup/init", `building CRS locally from ${merchants.length} merchants`);
    const crsResult = await buildCrs({
      merchants: merchants.map((m: any) => ({ name: m.name, origin: m.origin })),
      setSize: BENEFICIARY_CAPACITY,
    });
    const crsHex = crsResult.crs_hex;
    setCrs(crsHex);

    setAnonymitySet({
      commitments: [],
      finalized: false,
      count: 0,
      capacity: BENEFICIARY_CAPACITY,
    });
    setPhase(0);
    log("setup/init", `initialized: ${merchants.length} merchants, CRS=${crsHex.length} hex chars`);

    // Spawn any pending merchant gRPC servers
    spawnPendingMerchants();

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
