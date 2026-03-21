"use client";

import { useState } from "react";
import { useToast } from "@/components/ToastProvider";
import { clearAllLocalState } from "@/lib/useLocalState";

function FundByAddress() {
  const { toast } = useToast();
  const [address, setAddress] = useState("");
  const [funding, setFunding] = useState(false);

  async function handleFund() {
    if (!address.trim()) return;
    setFunding(true);
    try {
      const res = await fetch("/api/wallet/faucet", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ address: address.trim() }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      const amt = data.amount_sats ? `${(data.amount_sats / 100_000_000).toFixed(2)} BTC` : "funds";
      toast(`Sent ${amt} to ${address.trim().slice(0, 12)}...`, "success");
      setAddress("");
    } catch (e: any) {
      toast(e.message || "Funding failed", "error");
    }
    setFunding(false);
  }

  return (
    <div
      className="card"
      style={{ marginBottom: "1.5rem" }}
    >
      <h3 style={{ fontWeight: 600, marginBottom: "0.25rem" }}>
        Fund
      </h3>
      <p style={{ color: "#888", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
        Paste any regtest address to send it coinbase rewards.
      </p>
      <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
        <input
          type="text"
          placeholder="bcrt1p..."
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleFund()}
          style={{
            flex: 1,
            minWidth: "200px",
            background: "#111",
            border: "1px solid #333",
            borderRadius: "0.5rem",
            padding: "0.5rem 0.75rem",
            color: "#fff",
            fontFamily: "var(--font-geist-mono)",
            fontSize: "0.85rem",
          }}
        />
        <button
          className="btn"
          onClick={handleFund}
          disabled={funding || !address.trim()}
          style={{ flexShrink: 0 }}
        >
          {funding ? "Mining..." : "Fund"}
        </button>
      </div>
    </div>
  );
}

export default function DemoPage() {
  const { toast } = useToast();
  const [loading, setLoading] = useState(false);

  async function handleReset() {
    setLoading(true);
    try {
      await fetch("/api/reset", { method: "POST" });
      clearAllLocalState();
      toast("Demo state reset — all wallets and processes cleared", "success");
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
        Manage your demo environment &mdash; fund wallets or reset state.
      </p>

      {/* Fund by Address */}
      <FundByAddress />

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
