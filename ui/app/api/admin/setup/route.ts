// POST /api/admin/setup — Phase 0: register merchants, create set, fetch CRS

import { NextResponse } from "next/server";
import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { setMerchants, setCrs, setAnonymitySet, setPhase, getState } from "@/lib/state";

const DEFAULT_MERCHANTS = [
  { name: "CoffeeCo", origin: "https://coffeeco.com" },
  { name: "BookStore", origin: "https://bookstore.com" },
  { name: "TechMart", origin: "https://techmart.com" },
];

const SET_CAPACITY = 8;

export async function POST() {
  try {
    const registry = getRegistryClient();

    // Register merchants
    for (const m of DEFAULT_MERCHANTS) {
      await grpcCall(registry, "RegisterMerchant", {
        name: m.name,
        origin: m.origin,
        email: "",
        phone: "",
      });
    }

    // Create anonymity set
    const state = getState();
    await grpcCall(registry, "CreateSet", {
      set_id: state.set_id,
      merchant_names: DEFAULT_MERCHANTS.map((m) => m.name),
      beneficiary_capacity: SET_CAPACITY,
    });

    // Fetch merchants list
    const merchantsResp: any = await grpcCall(registry, "GetMerchants", {});
    const merchants = (merchantsResp.merchants || []).map((m: any) => ({
      name: m.name,
      origin: m.origin,
      credential_generator: Buffer.from(m.credential_generator).toString("hex"),
    }));
    setMerchants(merchants);

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
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
