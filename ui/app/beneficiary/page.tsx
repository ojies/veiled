"use client";

import { useState, useEffect, useCallback, useMemo } from "react";
import Stepper from "@/components/Stepper";
import PhaseCard from "@/components/PhaseCard";
import WalletCard from "@/components/WalletCard";
import HexDisplay from "@/components/HexDisplay";
import { useToast } from "@/components/ToastProvider";
import { useSessionState } from "@/lib/useLocalState";

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
  const [tabIndex] = useState(() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("tab");
  });

  // Persisted state (per-tab via sessionStorage, keyed by tab index)
  const tabKey = tabIndex ? `:${tabIndex}` : "";
  const [name, setName] = useSessionState(`ben:name${tabKey}`, tabIndex ? `Beneficiary ${tabIndex}` : "");
  const [phi, setPhi] = useSessionState<string | null>(`ben:phi${tabKey}`, null);
  const [regIndex, setRegIndex] = useSessionState<number | null>(`ben:regIndex${tabKey}`, null);
  const [setStatus, setSetStatus] = useSessionState<{ count: number; capacity: number } | null>(`ben:setStatus${tabKey}`, null);
  const [finalized, setFinalized] = useSessionState(`ben:finalized${tabKey}`, false);
  const [registrations, setRegistrations] = useSessionState<Registration[]>(`ben:registrations${tabKey}`, []);
  const [payments, setPayments] = useSessionState<Payment[]>(`ben:payments${tabKey}`, []);
  const [walletAddress, setWalletAddress] = useSessionState(`ben:walletAddr${tabKey}`, "");
  const [walletMnemonic, setWalletMnemonic] = useSessionState(`ben:walletMnemonic${tabKey}`, "");
  const [walletCreated, setWalletCreated] = useSessionState(`ben:walletCreated${tabKey}`, false);
  const [registryAddress, setRegistryAddress] = useSessionState(`ben:registryAddr${tabKey}`, "");

  // Fee config from server
  const [fees, setFees] = useState<{ beneficiary: number; merchant: number } | null>(null);

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

  // Memoize wallet name to avoid recomputing on every render
  const walletName = useMemo(
    () => name ? `beneficiary-${name.toLowerCase().replace(/\s+/g, "-")}` : "",
    [name]
  );

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

  // Check init status (called on mount and before credential creation)
  const checkInit = useCallback(async () => {
    try {
      const res = await fetch("/api/setup/init", { method: "POST" });
      const data = await res.json();
      if (data.fees) setFees(data.fees);
      if (data.waiting) {
        setInitWaiting(true);
        setInitDone(false);
        return false;
      } else {
        setInitWaiting(false);
        setInitDone(true);
        setRegistryAddress(data.registry_address || "");
        if (data.merchants) {
          setMerchants(data.merchants);
          if (data.merchants.length) setSelectedMerchant(data.merchants[0].name);
        }
        return true;
      }
    } catch {
      toast("Failed to connect to backend", "error");
      return false;
    }
  }, [toast, setRegistryAddress]);

  // Check on mount + poll with delay when waiting
  useEffect(() => {
    checkInit();
  }, [checkInit]);

  useEffect(() => {
    if (!initWaiting) return;
    const interval = setInterval(checkInit, 10_000);
    return () => clearInterval(interval);
  }, [initWaiting, checkInit]);

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
    if (!walletName) return;
    try {
      const res = await fetch(
        `/api/wallet/balance?name=${encodeURIComponent(walletName)}`
      );
      const data = await res.json();
      setWalletBalance(data.total || 0);
    } catch {
      // ignore
    }
  }, [walletName]);

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

  async function createWallet() {
    if (!walletName) return;
    setWalletLoading(true);
    try {
      const w = await api("/api/wallet/create", { name: walletName, role: "beneficiary" });
      setWalletAddress(w.address);
      setWalletMnemonic(w.mnemonic || "");
      setWalletCreated(true);
      toast("Wallet created", "success");
    } catch (e: any) {
      toast(`Wallet: ${e.message}`, "error");
    }
    setWalletLoading(false);
  }

  async function createCredential() {
    if (!name.trim()) return;
    setLoading("credential");
    // Re-check merchant status on demand
    const ready = await checkInit();
    if (!ready) {
      toast("Not enough merchants registered in the registry. Register merchants first or use the seed merchant faucet on the Demo Controls page.", "error");
      setLoading("");
      return;
    }
    try {
      const data = await api("/api/beneficiary/credential", { name: name.trim() });
      setPhi(data.phi);
      toast("Identity credential created", "success");

      // Auto-create wallet if not already created
      if (!walletCreated) {
        await createWallet();
      }
    } catch (e: any) {
      toast(e.message, "error");
    }
    setLoading("");
  }

  async function fundWallet() {
    if (!walletName) return;
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
      // Check balance before attempting registration
      if (fees && walletBalance < fees.beneficiary) {
        toast(
          `Insufficient balance: need ${fees.beneficiary.toLocaleString()} sats, have ${walletBalance.toLocaleString()}. Fund your wallet first.`,
          "error"
        );
        setLoading("");
        return;
      }

      const data = await api("/api/beneficiary/register", { name: name.trim() });
      setRegIndex(data.index);
      setSetStatus({ count: data.set_count, capacity: data.set_capacity });
      toast("Registered with anonymity set", "success");
      await refreshBalance();

      if (data.set_count >= data.set_capacity) {
        await finalizeSet();
      }
    } catch (e: any) {
      const msg = e.message || "Registration failed";
      if (msg.includes("already registered")) {
        toast("Already registered in this set", "error");
      } else if (msg.includes("amount too low") || msg.includes("insufficient") || msg.includes("not enough")) {
        toast("Insufficient funds for registration fee. Use the faucet to top up.", "error");
      } else if (msg.includes("full")) {
        toast("Anonymity set is full", "error");
      } else if (msg.includes("UNAVAILABLE") || msg.includes("connect")) {
        toast("Cannot reach registry server", "error");
      } else {
        toast(msg, "error");
      }
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

  const merchantsReady = initDone && !initWaiting;

  return (
    <div className="fade-in">
      <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
        Beneficiary Dashboard
      </h1>
      <p style={{ color: "#666", marginBottom: "1.5rem" }}>
        Create your credential, register, and receive payments
      </p>

      <Stepper steps={STEPS} activeStep={activeStep} />

      {!merchantsReady && !phi && (
        <div className="alert-banner alert-banner--warning">
          Waiting for merchants to register in the registry. Register merchants from the{" "}
          <a href="/merchant">Merchant</a> page or use the seed faucet on the{" "}
          <a href="/demo">Demo Controls</a> page.
        </div>
      )}

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

      {/* Wallet — create independently of credential/merchant status */}
      {name.trim() && !walletCreated && (
        <div style={{
          background: "#0a0a0a",
          border: "1px solid #222",
          borderRadius: "0.75rem",
          padding: "1rem 1.25rem",
          marginBottom: "1rem",
        }}>
          <p style={{ color: "#888", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
            Create a wallet to receive and send funds. This can be done at any time.
          </p>
          <button className="btn" onClick={createWallet} disabled={walletLoading}>
            {walletLoading ? "Creating..." : "Create Wallet"}
          </button>
        </div>
      )}
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
            <p style={{ color: "#888", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
              Registration fee: {fees ? fees.beneficiary.toLocaleString() : "..."} sats to registry
            </p>
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
