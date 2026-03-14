// Centralized configuration for the Veiled UI.
// All values can be overridden via environment variables.

function intEnv(key: string, fallback: number): number {
  const v = process.env[key];
  if (v === undefined || v === "") return fallback;
  const n = parseInt(v, 10);
  return isNaN(n) ? fallback : n;
}

// ── Registry / Protocol ──

/** Minimum number of merchants required before a set can be created */
export const MIN_MERCHANTS = intEnv("MIN_MERCHANTS", 2);

/** Number of beneficiary slots in each anonymity set (must be a power of 2) */
export const BENEFICIARY_CAPACITY = intEnv("BENEFICIARY_CAPACITY", 4);

/** Starting port for dynamically spawned merchant gRPC servers */
export const MERCHANT_START_PORT = intEnv("MERCHANT_START_PORT", 50061);

/** Milliseconds to wait after spawning a merchant server */
export const MERCHANT_STARTUP_DELAY = intEnv("MERCHANT_STARTUP_DELAY", 1500);

/** Number of blocks mined to mature coinbase outputs */
export const MATURITY_BLOCKS = intEnv("MATURITY_BLOCKS", 10);
