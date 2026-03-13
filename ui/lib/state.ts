// Server-side in-memory simulation state (singleton across API routes)

import type { SimState, BeneficiaryState, MerchantInfo, AnonymitySet, Credential } from "./types";

const state: SimState = {
  phase: -1,
  merchants: [],
  crs_hex: null,
  anonymity_set: null,
  beneficiaries: {},
  set_id: 1,
};

export function getState(): SimState {
  return state;
}

export function resetState(): void {
  state.phase = -1;
  state.merchants = [];
  state.crs_hex = null;
  state.anonymity_set = null;
  state.beneficiaries = {};
  state.set_id = 1;
}

export function setPhase(phase: number): void {
  if (phase > state.phase) state.phase = phase;
}

export function setMerchants(merchants: MerchantInfo[]): void {
  state.merchants = merchants;
}

export function setCrs(crs_hex: string): void {
  state.crs_hex = crs_hex;
}

export function setAnonymitySet(set: AnonymitySet): void {
  state.anonymity_set = set;
}

export function addBeneficiary(name: string, credential: Credential): void {
  state.beneficiaries[name] = {
    name,
    credential,
    index: null,
    registered: false,
    registrations: [],
    payments: [],
  };
}

export function getBeneficiary(name: string): BeneficiaryState | undefined {
  return state.beneficiaries[name];
}

export function updateBeneficiary(
  name: string,
  update: Partial<BeneficiaryState>
): void {
  const ben = state.beneficiaries[name];
  if (ben) {
    Object.assign(ben, update);
  }
}
