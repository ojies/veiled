"use client";

import { useState, useEffect, useCallback } from "react";
import Stepper from "@/components/Stepper";
import PhaseCard from "@/components/PhaseCard";
import WalletCard from "@/components/WalletCard";
import HexDisplay from "@/components/HexDisplay";
import { useToast } from "@/components/ToastProvider";
import { useLocalState } from "@/lib/useLocalState";

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

interface IncomingTx {
  txid: string;
  amount_sats: number;
  direction: string;
  confirmations: number;
  address: string;
}

const STEPS = [
  { label: "Create Identity" },
  { label: "Register" },
  { label: "Fund" },
  { label: "Connect to Merchant" },
  { label: "Request Payment" },
];

export default function BeneficiaryPage() {
  const { toast } = useToast();

  // Persisted state (survives page refresh)
  const [name, setName] = useLocalState("ben:name", "");
  const [phi, setPhi] = useLocalState<string | null>("ben:phi", null);
  const [regIndex, setRegIndex] = useLocalState<number | null>("ben:regIndex", null);
  const [setStatus, setSetStatus] = useLocalState<{ count: number; capacity: number } | null>("ben:setStatus", null);
  const [finalized, setFinalized] = useLocalState("ben:finalized", false);
  const [registrations, setRegistrations] = useLocalState<Registration[]>("ben:registrations", []);
  const [payments, setPayments] = useLocalState<Payment[]>("ben:payments", []);
  const [walletAddress, setWalletAddress] = useLocalState("ben:walletAddr", "");
  const [walletMnemonic, setWalletMnemonic] = useLocalState("ben:walletMnemonic", "");
  const [walletCreated, setWalletCreated] = useLocalState("ben:walletCreated", false);
  const [registryAddress, setRegistryAddress] = useLocalState("ben:registryAddr", "");

  // Ephemeral state (no persistence needed)
  const [merchants, setMerchants] = useState<Merchant[]>([]);
  const [selectedMerchant, setSelectedMerchant] = useState("");
  const [payMerchant, setPayMerchant] = useState("");
  const [payAmount, setPayAmount] = useState("5000");
  const [loading, setLoading] = useState("");
  const [initDone, setInitDone] = useState(false);
  const [initWaiting, setInitWaiting] = useState(false);
  const [walletBalance, setWalletBalance] = useState(0);
  const [walletLoading, setWalletLoading] = useState(false);

  // Incoming payments
  const [incomingTxs, setIncomingTxs] = useState<IncomingTx[]>([]);

  // Calculate active step
  const activeStep = !phi
    ? 0
    : regIndex === null
    ? 1
    : !finalized
    ? 2
    : registrations.filter((r) => r.status === "verified").length === 0
    ? 3
    : 4;

  // Lazy init on mount
  useEffect(() => {
    const init = async () => {
      try {
        const res = await fetch("/api/setup/init", { method: "POST" });
        const data = await res.json();
        if (data.waiting) {
          setInitWaiting(true);
        } else {
          setInitDone(true);
          setRegistryAddress(data.registry_address || "");
        }
        if (data.merchants) {
          setMerchants(data.merchants);
          if (data.merchants.length) setSelectedMerchant(data.merchants[0].name);
        }
      } catch {
        toast("Failed to connect to backend", "error");
      }
    };
    init();
  }, [toast]);

  // Fetch merchants periodically if waiting
  useEffect(() => {
    if (!initWaiting) return;
    const interval = setInterval(async () => {
      const res = await fetch("/api/setup/init", { method: "POST" });
      const data = await res.json();
      if (!data.waiting) {
        setInitWaiting(false);
        setInitDone(true);
        setRegistryAddress(data.registry_address || "");
        if (data.merchants) {
          setMerchants(data.merchants);
          if (data.merchants.length) setSelectedMerchant(data.merchants[0].name);
        }
        toast("Merchants registered — ready to proceed", "success");
      }
    }, 3000);
    return () => clearInterval(interval);
  }, [initWaiting, toast]);

  // Poll incoming payments
  useEffect(() => {
    if (!walletCreated || !name) return;
    const poll = async () => {
      try {
        const res = await fetch(
          `/api/beneficiary/incoming?name=${encodeURIComponent(name)}`
        );
        const data = await res.json();
        setIncomingTxs(data.transactions || []);
      } catch {
        // ignore
      }
    };
    poll();
    const interval = setInterval(poll, 3000);
    return () => clearInterval(interval);
  }, [walletCreated, name]);

  // Refresh balance
  const refreshBalance = useCallback(async () => {
    if (!name) return;
    const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;
    try {
      const res = await fetch(
        `/api/wallet/balance?name=${encodeURIComponent(walletName)}`
      );
      const data = await res.json();
      setWalletBalance(data.total || 0);
    } catch {
      // ignore
    }
  }, [name]);

  useEffect(() => {
    if (walletCreated) {
      const interval = setInterval(refreshBalance, 5000);
      return () => clearInterval(interval);
    }
  }, [walletCreated, refreshBalance]);

  async function api(url: string, body: any) {
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (!res.ok || data.error) throw new Error(data.error || "Request failed");
    return data;
  }

  async function createCredential() {
    if (!name.trim()) return;
    setLoading("credential");
    try {
      const data = await api("/api/beneficiary/credential", { name: name.trim() });
      setPhi(data.phi);
      toast("Identity credential created", "success");

      // Auto-create wallet
      const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;
      try {
        const w = await api("/api/wallet/create", { name: walletName, role: "beneficiary" });
        setWalletAddress(w.address);
        setWalletMnemonic(w.mnemonic || "");
        setWalletCreated(true);
      } catch (e: any) {
        toast(`Wallet: ${e.message}`, "error");
      }
    } catch (e: any) {
      toast(e.message, "error");
    }
    setLoading("");
  }

  async function fundWallet() {
    const walletName = `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}`;
    setWalletLoading(true);
    try {
      await api("/api/wallet/faucet", { names: [walletName] });
      toast("Wallet funded via regtest mining", "success");
      await refreshBalance();
    } catch (e: any) {
      toast(e.message, "error");
    }
    setWalletLoading(false);
  }

  async function registerWithRegistry() {
    setLoading("register");
    try {
      // Payment is handled internally by the register API route
      const data = await api("/api/beneficiary/register", { name: name.trim() });
      setRegIndex(data.index);
      setSetStatus({ count: data.set_count, capacity: data.set_capacity });
      toast("Registered with anonymity set", "success");

      if (data.set_count >= data.set_capacity) {
        await finalizeSet();
      }
    } catch (e: any) {
      toast(e.message, "error");
    }
    setLoading("");
  }

  async function finalizeSet() {
    setLoading("finalize");
    try {
      await api("/api/beneficiary/finalize", {});
      setFinalized(true);
      toast("Anonymity set finalized with real Bitcoin UTXO", "success");
    } catch (e: any) {
      if (e.message?.includes("already")) {
        setFinalized(true);
      } else {
        toast(e.message, "error");
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
      toast(`ZK proof verified by ${selectedMerchant}`, "success");
    } catch (e: any) {
      toast(e.message, "error");
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
      toast(`Payment of ${payAmount} sats requested from ${payMerchant}`, "success");
    } catch (e: any) {
      toast(e.message, "error");
    }
    setLoading("");
  }

  const registeredMerchants = registrations
    .filter((r) => r.status === "verified")
    .map((r) => r.merchant_name);

  if (initWaiting) {
    return (
      <div>
        <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
          Beneficiary Flow
        </h1>
        <div className="card" style={{ textAlign: "center", padding: "3rem" }}>
          <p style={{ color: "#f5a623", fontSize: "1.1rem" }}>
            Waiting for merchants to register...
          </p>
          <p style={{ color: "#666", marginTop: "0.5rem" }}>
            Open a new tab and create a merchant first.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="fade-in">
      <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
        Beneficiary Flow
      </h1>
      <p style={{ color: "#666", marginBottom: "1.5rem" }}>
        Create your credential, register, and receive payments
      </p>

      <Stepper steps={STEPS} activeStep={activeStep} />

      {/* Create Identity */}
      <PhaseCard
        title="Create Identity"
        active={activeStep === 0}
        completed={!!phi}
        defaultOpen={!phi}
      >
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
              {loading === "credential" ? "Creating..." : "Create"}
            </button>
          </div>
        ) : (
          <div>
            <p>
              Credential for <strong>{name}</strong> created.
            </p>
            <div style={{ marginTop: "0.5rem" }}>
              <HexDisplay value={phi} label="Commitment (Phi)" />
            </div>
          </div>
        )}
      </PhaseCard>

      {/* Wallet */}
      {walletCreated && (
        <WalletCard
          name={`${name}'s Wallet`}
          address={walletAddress}
          balance={walletBalance}
          mnemonic={walletMnemonic}
          onFaucet={fundWallet}
          loading={walletLoading}
        />
      )}

      {/* Register */}
      <PhaseCard
        title="Register"
        active={activeStep === 1}
        completed={regIndex !== null}
        locked={!phi}
      >
        {regIndex === null ? (
          <div>
            {registryAddress && (
              <p style={{ color: "#888", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
                Registration fee: 10,000 sats to registry
              </p>
            )}
            <button className="btn" onClick={registerWithRegistry} disabled={!!loading}>
              {loading === "register" ? "Registering..." : "Pay & Register"}
            </button>
          </div>
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
                Waiting for more beneficiaries ({setStatus.capacity - setStatus.count} remaining)...
              </p>
            )}
            {!finalized && setStatus && setStatus.count >= setStatus.capacity && (
              <button className="btn" onClick={finalizeSet} disabled={!!loading} style={{ marginTop: "0.5rem" }}>
                {loading === "finalize" ? "Finalizing..." : "Finalize Set"}
              </button>
            )}
            {finalized && (
              <p style={{ color: "#4ade80", marginTop: "0.5rem" }}>
                Set finalized — anonymity set sealed
              </p>
            )}
          </div>
        )}
      </PhaseCard>

      {/* Connect to Merchant */}
      <PhaseCard
        title="Connect to Merchant"
        active={activeStep === 3}
        completed={registeredMerchants.length > 0}
        locked={!finalized}
      >
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
            {loading === "payment-id" ? "Proving..." : "Register"}
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
                      <HexDisplay value={r.pseudonym} />
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
                <HexDisplay value={r.pseudonym} />
              </div>
            ))}
          </div>
        )}
      </PhaseCard>

      {/* Request Payment */}
      <PhaseCard
        title="Request Payment"
        active={activeStep === 4}
        completed={payments.length > 0}
        locked={registeredMerchants.length === 0}
      >
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
            {loading === "payment" ? "Authenticating..." : "Request"}
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
                      <HexDisplay value={p.address} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </PhaseCard>

      {/* Incoming Payments */}
      {walletCreated && incomingTxs.length > 0 && (
        <PhaseCard title="Incoming Payments" defaultOpen active>
          <div className="table-wrap">
            <table style={{ width: "100%", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                  <th style={{ padding: "0.5rem" }}>TxID</th>
                  <th style={{ padding: "0.5rem" }}>Amount</th>
                  <th style={{ padding: "0.5rem" }}>Confirmations</th>
                </tr>
              </thead>
              <tbody>
                {incomingTxs.map((tx, i) => (
                  <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                    <td style={{ padding: "0.5rem" }}>
                      <HexDisplay value={tx.txid} />
                    </td>
                    <td style={{ padding: "0.5rem" }}>{tx.amount_sats} sats</td>
                    <td style={{ padding: "0.5rem" }}>{tx.confirmations}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </PhaseCard>
      )}
    </div>
  );
}
