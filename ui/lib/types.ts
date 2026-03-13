// Shared TypeScript types for the Veiled UI

export interface Credential {
  phi: string; // 66-char hex (33 bytes compressed point)
  sk: string; // 64-char hex
  r: string; // 64-char hex
  k: string; // 64-char hex
  friendly_name: string;
}

export interface MerchantInfo {
  name: string;
  origin: string;
  credential_generator: string; // 66-char hex
}

export interface BeneficiaryState {
  name: string;
  credential: Credential | null;
  index: number | null;
  registered: boolean;
  registrations: PaymentIdRegistration[];
  payments: PaymentResult[];
}

export interface PaymentIdRegistration {
  merchant_name: string;
  pseudonym: string; // 66-char hex
  nullifier: string; // 66-char hex
  status: "pending" | "verified" | "failed";
}

export interface PaymentResult {
  merchant_name: string;
  amount: number;
  address: string; // P2TR bc1p...
  friendly_name: string;
}

export interface AnonymitySet {
  commitments: string[]; // array of 66-char hex
  finalized: boolean;
  count: number;
  capacity: number;
}

export interface SimState {
  phase: number;
  merchants: MerchantInfo[];
  crs_hex: string | null;
  anonymity_set: AnonymitySet | null;
  beneficiaries: Record<string, BeneficiaryState>;
  set_id: number;
}
