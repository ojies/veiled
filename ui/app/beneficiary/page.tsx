"use client";

import { useState, useEffect } from "react";

interface Merchant {
  name: string;
}

interface Registration {
  merchant_name: string;
  pseudonym: string;
  nullifier: string;
  status: string;
}

interface Payment {
  merchant_name: string;
  amount: number;
  address: string;
  friendly_name: string;
}

function truncHex(hex: string, len = 16): string {
  if (hex.length <= len * 2) return hex;
  return hex.slice(0, len) + "…" + hex.slice(-8);
}

export default function BeneficiaryPage() {
  const [name, setName] = useState("");
  const [phi, setPhi] = useState<string | null>(null);
  const [regIndex, setRegIndex] = useState<number | null>(null);
  const [setStatus, setSetStatus] = useState<{ count: number; capacity: number } | null>(null);
  const [finalized, setFinalized] = useState(false);
  const [merchants, setMerchants] = useState<Merchant[]>([]);
  const [selectedMerchant, setSelectedMerchant] = useState("");
  const [registrations, setRegistrations] = useState<Registration[]>([]);
  const [payMerchant, setPayMerchant] = useState("");
  const [payAmount, setPayAmount] = useState("5000");
  const [payments, setPayments] = useState<Payment[]>([]);
  const [loading, setLoading] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    fetch("/api/beneficiary/merchants")
      .then((r) => r.json())
      .then((d) => {
        setMerchants(d.merchants || []);
        if (d.merchants?.length) setSelectedMerchant(d.merchants[0].name);
      });
  }, []);

  async function api(url: string, body: any) {
    setError("");
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (!res.ok) throw new Error(data.error);
    return data;
  }

  async function createCredential() {
    if (!name.trim()) return;
    setLoading("credential");
    try {
      const data = await api("/api/beneficiary/credential", { name: name.trim() });
      setPhi(data.phi);
    } catch (e: any) {
      setError(e.message);
    }
    setLoading("");
  }

  async function registerWithRegistry() {
    setLoading("register");
    try {
      const data = await api("/api/beneficiary/register", { name: name.trim() });
      setRegIndex(data.index);
      setSetStatus({ count: data.set_count, capacity: data.set_capacity });
      if (data.set_count >= data.set_capacity) {
        // Auto-finalize when full
        await finalize();
      }
    } catch (e: any) {
      setError(e.message);
    }
    setLoading("");
  }

  async function finalize() {
    setLoading("finalize");
    try {
      await api("/api/admin/finalize", {});
      setFinalized(true);
    } catch (e: any) {
      // Might already be finalized
      if (e.message?.includes("already")) {
        setFinalized(true);
      } else {
        setError(e.message);
      }
    }
    setLoading("");
  }

  async function registerPaymentId() {
    if (!selectedMerchant) return;
    setLoading("payment-id");
    try {
      const data = await api("/api/beneficiary/payment-id", {
        beneficiary: name.trim(),
        merchant: selectedMerchant,
      });
      setRegistrations((prev) => [...prev, data]);
      if (!payMerchant) setPayMerchant(selectedMerchant);
    } catch (e: any) {
      setError(e.message);
    }
    setLoading("");
  }

  async function requestPayment() {
    if (!payMerchant || !payAmount) return;
    setLoading("payment");
    try {
      const data = await api("/api/beneficiary/payment", {
        beneficiary: name.trim(),
        merchant: payMerchant,
        amount: parseInt(payAmount),
      });
      setPayments((prev) => [...prev, data]);
    } catch (e: any) {
      setError(e.message);
    }
    setLoading("");
  }

  const registeredMerchants = registrations
    .filter((r) => r.status === "verified")
    .map((r) => r.merchant_name);

  return (
    <div>
      <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
        Beneficiary Flow
      </h1>
      <p style={{ color: "#666", marginBottom: "2rem" }}>
        Create your credential, register, and receive payments
      </p>

      {error && (
        <div className="card" style={{ borderColor: "#dc2626", marginBottom: "1rem", color: "#f87171" }}>
          {error}
        </div>
      )}

      {/* Phase 1: Create Credential */}
      <section className="card" style={{ marginBottom: "1.5rem" }}>
        <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
          Phase 1 — Create Credential
        </h2>
        {!phi ? (
          <div className="form-row">
            <input
              type="text"
              placeholder="Enter your name (e.g., alice)"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createCredential()}
              style={{
                background: "#111",
                border: "1px solid #333",
                borderRadius: "0.5rem",
                padding: "0.5rem 0.75rem",
                color: "#fff",
                flex: 1,
                minWidth: 0,
              }}
            />
            <button className="btn" onClick={createCredential} disabled={!!loading || !name.trim()}>
              {loading === "credential" ? "Creating…" : "Create"}
            </button>
          </div>
        ) : (
          <div>
            <p>
              Credential for <strong>{name}</strong> created.
            </p>
            <p style={{ marginTop: "0.5rem" }}>
              Φ = <span className="hex">{truncHex(phi)}</span>
            </p>
          </div>
        )}
      </section>

      {/* Phase 2: Register */}
      {phi && (
        <section className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
            Phase 2 — Register with Registry
          </h2>
          {regIndex === null ? (
            <button className="btn" onClick={registerWithRegistry} disabled={!!loading}>
              {loading === "register" ? "Registering…" : "Register Φ"}
            </button>
          ) : (
            <div>
              <p>
                Registered at index <strong>{regIndex}</strong>
              </p>
              {setStatus && (
                <p style={{ color: "#666" }}>
                  Set: {setStatus.count}/{setStatus.capacity} members
                </p>
              )}
              {!finalized && setStatus && setStatus.count < setStatus.capacity && (
                <p style={{ color: "#f5a623", marginTop: "0.5rem" }}>
                  Waiting for more beneficiaries to register ({setStatus.capacity - setStatus.count} remaining)…
                </p>
              )}
              {!finalized && setStatus && setStatus.count >= setStatus.capacity && (
                <button className="btn" onClick={finalize} disabled={!!loading} style={{ marginTop: "0.5rem" }}>
                  {loading === "finalize" ? "Finalizing…" : "Finalize Set"}
                </button>
              )}
              {finalized && (
                <p style={{ color: "#4ade80", marginTop: "0.5rem" }}>
                  ✓ Set finalized — anonymity set sealed
                </p>
              )}
            </div>
          )}
        </section>
      )}

      {/* Phase 3-4: Register Payment Identity */}
      {finalized && (
        <section className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
            Phase 3-4 — Register Payment Identity
          </h2>
          <div className="form-row" style={{ marginBottom: "1rem" }}>
            <select
              value={selectedMerchant}
              onChange={(e) => setSelectedMerchant(e.target.value)}
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
            <button
              className="btn"
              onClick={registerPaymentId}
              disabled={!!loading || registeredMerchants.includes(selectedMerchant)}
            >
              {loading === "payment-id" ? "Proving…" : "Register"}
            </button>
          </div>

          {registrations.length > 0 && (
            <div className="table-wrap">
            <table style={{ width: "100%", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                  <th style={{ padding: "0.5rem" }}>Merchant</th>
                  <th style={{ padding: "0.5rem" }}>Pseudonym</th>
                  <th style={{ padding: "0.5rem" }}>Status</th>
                </tr>
              </thead>
              <tbody>
                {registrations.map((r, i) => (
                  <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                    <td style={{ padding: "0.5rem" }}>{r.merchant_name}</td>
                    <td style={{ padding: "0.5rem" }}>
                      <span className="hex">{truncHex(r.pseudonym)}</span>
                    </td>
                    <td style={{ padding: "0.5rem" }}>
                      <span className="badge badge-success">{r.status}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            </div>
          )}

          {/* Unlinkability demo */}
          {registrations.length >= 2 && (
            <div
              style={{
                marginTop: "1.5rem",
                padding: "1rem",
                background: "#111",
                borderRadius: "0.5rem",
                border: "1px solid #333",
              }}
            >
              <h3 style={{ fontWeight: 600, marginBottom: "0.5rem", fontSize: "0.9rem" }}>
                Cross-Merchant Unlinkability
              </h3>
              <p style={{ color: "#999", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
                Your pseudonyms are cryptographically different at each merchant — they cannot be linked:
              </p>
              {registrations.map((r, i) => (
                <div key={i} style={{ marginBottom: "0.25rem" }}>
                  <span style={{ color: "#999", display: "inline-block", width: "100px" }}>
                    {r.merchant_name}:
                  </span>
                  <span className="hex">{r.pseudonym}</span>
                </div>
              ))}
            </div>
          )}
        </section>
      )}

      {/* Phase 5: Payment Request */}
      {registeredMerchants.length > 0 && (
        <section className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontWeight: 600, marginBottom: "0.75rem", color: "#f5a623" }}>
            Phase 5 — Request Payment
          </h2>
          <div className="form-row" style={{ marginBottom: "1rem" }}>
            <select
              value={payMerchant}
              onChange={(e) => setPayMerchant(e.target.value)}
              style={{
                background: "#111",
                border: "1px solid #333",
                borderRadius: "0.5rem",
                padding: "0.5rem 0.75rem",
                color: "#fff",
              }}
            >
              {registeredMerchants.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            <input
              type="number"
              value={payAmount}
              onChange={(e) => setPayAmount(e.target.value)}
              placeholder="Amount (sats)"
              style={{
                background: "#111",
                border: "1px solid #333",
                borderRadius: "0.5rem",
                padding: "0.5rem 0.75rem",
                color: "#fff",
                width: "120px",
              }}
            />
            <button className="btn" onClick={requestPayment} disabled={!!loading}>
              {loading === "payment" ? "Authenticating…" : "Request"}
            </button>
          </div>

          {payments.length > 0 && (
            <div className="table-wrap">
            <table style={{ width: "100%", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                  <th style={{ padding: "0.5rem" }}>Merchant</th>
                  <th style={{ padding: "0.5rem" }}>Amount</th>
                  <th style={{ padding: "0.5rem" }}>P2TR Address</th>
                </tr>
              </thead>
              <tbody>
                {payments.map((p, i) => (
                  <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                    <td style={{ padding: "0.5rem" }}>{p.merchant_name}</td>
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
        </section>
      )}
    </div>
  );
}
