"use client";

import { useState, useEffect, useCallback } from "react";

interface Identity {
  beneficiary: string;
  pseudonym: string;
  nullifier: string;
}

interface Payment {
  beneficiary: string;
  amount: number;
  address: string;
}

interface Merchant {
  name: string;
  origin: string;
}

function truncHex(hex: string, len = 16): string {
  if (hex.length <= len * 2) return hex;
  return hex.slice(0, len) + "…" + hex.slice(-8);
}

export default function MerchantPage() {
  const [merchants, setMerchants] = useState<Merchant[]>([]);
  const [selected, setSelected] = useState("");
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [payments, setPayments] = useState<Payment[]>([]);
  const [autoRefresh, setAutoRefresh] = useState(true);

  useEffect(() => {
    fetch("/api/beneficiary/merchants")
      .then((r) => r.json())
      .then((d) => {
        const m = d.merchants || [];
        setMerchants(m);
        if (m.length) setSelected(m[0].name);
      });
  }, []);

  const refresh = useCallback(async () => {
    if (!selected) return;
    const [idRes, payRes] = await Promise.all([
      fetch(`/api/merchant/identities?merchant=${selected}`),
      fetch(`/api/merchant/payments?merchant=${selected}`),
    ]);
    const idData = await idRes.json();
    const payData = await payRes.json();
    setIdentities(idData.identities || []);
    setPayments(payData.payments || []);
  }, [selected]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(refresh, 3000);
    return () => clearInterval(interval);
  }, [autoRefresh, refresh]);

  return (
    <div>
      <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
        Merchant Dashboard
      </h1>
      <p style={{ color: "#666", marginBottom: "2rem" }}>
        View registered identities and incoming payments
      </p>

      {/* Merchant selector */}
      <div className="form-row" style={{ marginBottom: "2rem" }}>
        <label style={{ color: "#999" }}>Merchant:</label>
        <select
          value={selected}
          onChange={(e) => setSelected(e.target.value)}
          style={{
            background: "#111",
            border: "1px solid #333",
            borderRadius: "0.5rem",
            padding: "0.5rem 0.75rem",
            color: "#fff",
          }}
        >
          {merchants.map((m) => (
            <option key={m.name} value={m.name}>
              {m.name}
            </option>
          ))}
        </select>
        <button className="btn-outline" onClick={refresh}>
          Refresh
        </button>
        <label style={{ color: "#999", fontSize: "0.85rem", display: "flex", alignItems: "center", gap: "0.35rem" }}>
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => setAutoRefresh(e.target.checked)}
          />
          Auto-refresh
        </label>
      </div>

      {/* Registered Identities */}
      <section className="card" style={{ marginBottom: "1.5rem" }}>
        <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
          Registered Identities (Phase 4)
        </h2>
        {identities.length === 0 ? (
          <p style={{ color: "#666" }}>
            No beneficiaries registered yet. Waiting for incoming registrations…
          </p>
        ) : (
          <div className="table-wrap">
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                <th style={{ padding: "0.5rem" }}>Beneficiary</th>
                <th style={{ padding: "0.5rem" }}>Pseudonym</th>
                <th style={{ padding: "0.5rem" }}>Nullifier</th>
              </tr>
            </thead>
            <tbody>
              {identities.map((id, i) => (
                <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                  <td style={{ padding: "0.5rem" }}>{id.beneficiary}</td>
                  <td style={{ padding: "0.5rem" }}>
                    <span className="hex">{truncHex(id.pseudonym)}</span>
                  </td>
                  <td style={{ padding: "0.5rem" }}>
                    <span className="hex">{truncHex(id.nullifier)}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        )}
        <p style={{ color: "#666", marginTop: "0.75rem", fontSize: "0.85rem" }}>
          {identities.length} registered {identities.length === 1 ? "identity" : "identities"}
        </p>
      </section>

      {/* Payment History */}
      <section className="card" style={{ marginBottom: "1.5rem" }}>
        <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
          Payment Requests (Phase 5)
        </h2>
        {payments.length === 0 ? (
          <p style={{ color: "#666" }}>
            No payment requests yet. Waiting for beneficiary requests…
          </p>
        ) : (
          <div className="table-wrap">
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                <th style={{ padding: "0.5rem" }}>Beneficiary</th>
                <th style={{ padding: "0.5rem" }}>Amount</th>
                <th style={{ padding: "0.5rem" }}>P2TR Address</th>
              </tr>
            </thead>
            <tbody>
              {payments.map((p, i) => (
                <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                  <td style={{ padding: "0.5rem" }}>{p.beneficiary}</td>
                  <td style={{ padding: "0.5rem" }}>{p.amount} sats</td>
                  <td style={{ padding: "0.5rem" }}>
                    <span className="hex">{p.address}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        )}
        <p style={{ color: "#666", marginTop: "0.75rem", fontSize: "0.85rem" }}>
          {payments.length} {payments.length === 1 ? "payment" : "payments"} processed
        </p>
      </section>

      {/* Privacy note */}
      <section
        className="card"
        style={{ borderColor: "#f5a62344" }}
      >
        <h2 style={{ fontWeight: 600, marginBottom: "0.5rem", fontSize: "0.95rem" }}>
          What you can see as a merchant
        </h2>
        <ul style={{ color: "#999", fontSize: "0.85rem", lineHeight: 1.8, paddingLeft: "1.25rem" }}>
          <li>Pseudonym — unique to your merchant, cannot be linked to other merchants</li>
          <li>Nullifier — prevents double-registration (Sybil resistance)</li>
          <li>Friendly name — revealed by the beneficiary (privacy trade-off)</li>
          <li>ZK proof verified — you know they&apos;re in the anonymity set, but not which position</li>
        </ul>
      </section>
    </div>
  );
}
