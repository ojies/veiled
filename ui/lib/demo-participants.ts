// Demo participant names — shared between client pages and simulation.
// Must be safe for "use client" components (no process.env).

/** Default merchant names by tab index (1-based). Matches simulation.rs. */
export const MERCHANT_NAMES: Record<number, { name: string; origin: string }> = {
  1: { name: "CoffeeCo", origin: "https://coffeeco.com" },
  2: { name: "BookStore", origin: "https://bookstore.com" },
};

/** Default beneficiary names by tab index (1-based). Matches simulation.rs. */
export const BENEFICIARY_NAMES: Record<number, string> = {
  1: "alice",
  2: "bob",
  3: "carol",
  4: "dave",
};
