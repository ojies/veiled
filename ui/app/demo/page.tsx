"use client";

import { useState, useEffect } from "react";
import FaucetButton from "@/components/FaucetButton";
import { useToast } from "@/components/ToastProvider";
import { clearAllLocalState } from "@/lib/useLocalState";

export default function DemoPage() {
  const { toast } = useToast();
  const [walletNames, setWalletNames] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [seedLoading, setSeedLoading] = useState(false);

  // Fetch current state to find all known wallets
  useEffect(() => {
    const fetchState = async () => {
      try {
        const res = await fetch("/api/state");
        const data = await res.json();
        const names: string[] = [];
        if (data.wallets) {
          names.push(...Object.keys(data.wallets));
        }
        // Always include registry
        if (!names.includes("registry")) names.push("registry");
        setWalletNames(names);
      } catch {
        setWalletNames(["registry"]);
      }
    };
    fetchState();
  }, []);

  async function handleSeedMerchant() {
    setSeedLoading(true);
    try {
      const res = await fetch("/api/setup/seed-merchants", { method: "POST" });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      if (data.existing) {
        toast("Seed merchant already exists", "info");
      } else {
        toast(`Seed merchant created on port ${data.port}`, "success");
      }
    } catch (e: any) {
      toast(e.message || "Seed merchant creation failed", "error");
    }
    setSeedLoading(false);
  }

  async function handleReset() {
    setLoading(true);
    try {
      await fetch("/api/reset", { method: "POST" });
      clearAllLocalState();
      toast("Demo state reset — all wallets and processes cleared", "success");
      setWalletNames(["registry"]);
    } catch {
      toast("Reset failed", "error");
    }
    setLoading(false);
  }

  return (
    <div className="fade-in" style={{ maxWidth: "700px", margin: "0 auto" }}>
      <h1
        style={{
          fontSize: "clamp(1.3rem, 5vw, 1.8rem)",
          fontWeight: 700,
          marginBottom: "0.5rem",
        }}
      >
        Demo Controls
      </h1>
      <p style={{ color: "#666", marginBottom: "2rem" }}>
        Manage your demo environment &mdash; seed merchants, fund wallets, or reset state.
      </p>

      {/* Seed Merchant Faucet */}
      <div
        className="card"
        style={{
          marginBottom: "1.5rem",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "1rem",
          flexWrap: "wrap",
        }}
      >
        <div>
          <h3 style={{ fontWeight: 600, marginBottom: "0.25rem" }}>
            Seed Merchant
          </h3>
          <p style={{ color: "#888", fontSize: "0.85rem" }}>
            Auto-create a funded, registered merchant so the beneficiary flow
            can proceed without manual setup.
          </p>
        </div>
        <button
          className="btn"
          onClick={handleSeedMerchant}
          disabled={seedLoading}
          style={{ flexShrink: 0 }}
        >
          {seedLoading ? "Creating..." : "Create Seed Merchant"}
        </button>
      </div>

      {/* Fund All Wallets */}
      <div
        className="card"
        style={{
          marginBottom: "1.5rem",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "1rem",
          flexWrap: "wrap",
        }}
      >
        <div>
          <h3 style={{ fontWeight: 600, marginBottom: "0.25rem" }}>
            Fund All Wallets
          </h3>
          <p style={{ color: "#888", fontSize: "0.85rem" }}>
            Mine regtest blocks to fund {walletNames.length} wallet(s) via
            coinbase rewards.
          </p>
          {walletNames.length > 0 && (
            <p
              style={{
                color: "#555",
                fontSize: "0.75rem",
                marginTop: "0.25rem",
              }}
            >
              {walletNames.join(", ")}
            </p>
          )}
        </div>
        <FaucetButton
          walletNames={walletNames}
          onComplete={() => toast("All wallets funded", "success")}
        />
      </div>

      {/* Reset */}
      <div
        className="card"
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "1rem",
          flexWrap: "wrap",
        }}
      >
        <div>
          <h3 style={{ fontWeight: 600, marginBottom: "0.25rem" }}>
            Reset Demo
          </h3>
          <p style={{ color: "#888", fontSize: "0.85rem" }}>
            Clear all state, kill merchant processes, and start fresh.
          </p>
        </div>
        <button
          className="btn-outline"
          onClick={handleReset}
          disabled={loading}
          style={{ flexShrink: 0 }}
        >
          {loading ? "Resetting..." : "Reset"}
        </button>
      </div>
    </div>
  );
}
