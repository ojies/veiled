"use client";

import { useState, useEffect, useCallback, useMemo } from "react";
import Stepper from "@/components/Stepper";
import PhaseCard from "@/components/PhaseCard";
import WalletCard from "@/components/WalletCard";
import HexDisplay from "@/components/HexDisplay";
import { useToast } from "@/components/ToastProvider";
import { useSessionState } from "@/lib/useLocalState";
import { BENEFICIARY_NAMES } from "@/lib/demo-participants";

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
  const defaultBenName = tabIndex ? (BENEFICIARY_NAMES[Number(tabIndex)] ?? "") : "";
  const [name, setName] = useSessionState(`ben:name${tabKey}`, defaultBenName);
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
  const [merchantIdInput, setMerchantIdInput] = useState("");
  const [registrationToken, setRegistrationToken] = useState<string | null>(null);
  const [paymentToken, setPaymentToken] = useState<string | null>(null);
  const [paymentAddress, setPaymentAddress] = useState<string | null>(null);
  const [addressSats, setAddressSats] = useState<number | null>(null);
  const [payMerchant, setPayMerchant] = useState("");
  const [payAmount, setPayAmount] = useState("5000");
  const [loading, setLoading] = useState("");
  const [initDone, setInitDone] = useState(false);
  const [initWaiting, setInitWaiting] = useState(false);
  const [walletBalance, setWalletBalance] = useState(0);
  const [walletLoading, setWalletLoading] = useState(false);

  // Incoming payments
  const [incomingTxs, setIncomingTxs] = useState<IncomingTx[]>([]);
  const [paymentUtxos, setPaymentUtxos] = useState<Record<string, { txid: string; amount_sats: number }[]>>({});

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
    : registrations.length === 0
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
          if (data.merchants.length && !payMerchant) setPayMerchant(data.merchants[0].name);
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

  // Stream anonymity set status via SSE when registered but not yet finalized.
  // Detects when another tab fills the set and triggers finalization.
  useEffect(() => {
    if (regIndex === null || finalized) return;
    const es = new EventSource("/api/beneficiary/set-stream");

    es.addEventListener("status", (e) => {
      try {
        const data = JSON.parse(e.data);
        setSetStatus({ count: data.count, capacity: data.capacity });
        // If set is full but not finalized, trigger finalization
        if (!data.finalized && data.count >= data.capacity) {
          fetch("/api/beneficiary/finalize", { method: "POST" })
            .then((r) => r.json())
            .then((fdata) => {
              if (fdata.finalized) {
                setFinalized(true);
                setSetStatus({ count: fdata.count, capacity: fdata.capacity });
                toast("Anonymity set finalized and sealed", "success");
              }
            })
            .catch(() => {}); // Another tab may be finalizing
        }
      } catch { /* ignore parse errors */ }
    });

    es.addEventListener("finalized", (e) => {
      try {
        const data = JSON.parse(e.data);
        setSetStatus({ count: data.count, capacity: data.capacity });
        setFinalized(true);
        toast("Anonymity set sealed", "success");
      } catch { /* ignore */ }
      es.close();
    });

    es.onerror = () => {
      // SSE connection lost — will auto-reconnect by browser
    };

    return () => es.close();
  }, [regIndex, finalized, toast, setFinalized, setSetStatus]);

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

  // Poll payment addresses for incoming UTXOs (txid + amount)
  useEffect(() => {
    if (payments.length === 0) return;
    const poll = async () => {
      const updates: Record<string, { txid: string; amount_sats: number }[]> = {};
      for (const p of payments) {
        try {
          const res = await fetch(`/api/wallet/scan-address?address=${encodeURIComponent(p.address)}`);
          const data = await res.json();
          updates[p.address] = (data.utxos ?? []).map((u: any) => ({
            txid: u.txid,
            amount_sats: Math.round(u.amount * 1e8),
          }));
        } catch { /* ignore */ }
      }
      setPaymentUtxos(updates);
    };
    poll();
    const interval = setInterval(poll, 5000);
    return () => clearInterval(interval);
  }, [payments]);

  // Refresh balance
  const refreshBalance = useCallback(async () => {
    if (!walletName) return;
    try {
      const res = await fetch(
        `/api/wallet/balance?name=${encodeURIComponent(walletName)}`
      );
      const data = await res.json();
      const newBal = data.total || 0;
      // Only update if balance increased or was already 0.
      // scantxoutset can return 0 during mining races — ignore false drops.
      setWalletBalance((prev: number) => (newBal >= prev ? newBal : prev));
    } catch {
      // ignore
    }
  }, [walletName]);

  useEffect(() => {
    if (walletCreated) {
      refreshBalance();
      const interval = setInterval(refreshBalance, 5000);
      const onVisible = () => {
        if (document.visibilityState === "visible") refreshBalance();
      };
      document.addEventListener("visibilitychange", onVisible);
      return () => {
        clearInterval(interval);
        document.removeEventListener("visibilitychange", onVisible);
      };
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
    const mid = parseInt(merchantIdInput.trim(), 10);
    if (isNaN(mid)) return;
    setLoading("payment-id");
    setRegistrationToken(null);
    try {
      const data = await api("/api/beneficiary/payment-id", {
        beneficiary: name.trim(),
        merchant_id: mid,
      });
      setRegistrationToken(data.registration_token);
      setRegistrations((prev) => [
        ...prev,
        {
          merchant_name: `Merchant #${mid}`,
          pseudonym: data.pseudonym,
          nullifier: data.nullifier,
          status: "pending" as const,
        },
      ]);
      toast("Registration token created — copy and send to merchant", "success");
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
      setPaymentToken(data.token ?? null);
      setPaymentAddress(data.address ?? null);
      setAddressSats(null);
      toast(`Payment token created — copy and send to ${payMerchant}`, "success");
    } catch (e: any) {
      toast(e.message, "error");
    }
    setLoading("");
  }

  async function checkPaymentAddress() {
    if (!paymentAddress) return;
    try {
      const res = await fetch(
        `/api/wallet/scan-address?address=${encodeURIComponent(paymentAddress)}`
      );
      const data = await res.json();
      setAddressSats(data.total_amount_sats ?? 0);
    } catch {
      // ignore
    }
  }

  const registeredMerchants = registrations.map((r) => r.merchant_name);

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
        <p style={{ color: "#888", fontSize: "0.85rem", marginBottom: "0.75rem" }}>
          Enter the Merchant ID shown in the merchant&apos;s dashboard, then create your registration token.
        </p>
        <div className="form-row" style={{ marginBottom: "1rem" }}>
          <input
            type="number"
            value={merchantIdInput}
            onChange={(e) => setMerchantIdInput(e.target.value)}
            placeholder="Merchant ID (e.g., 1)"
            style={{
              background: "#111",
              border: "1px solid #333",
              borderRadius: "0.5rem",
              padding: "0.5rem 0.75rem",
              color: "#fff",
              width: "160px",
            }}
          />
          <button
            className="btn"
            onClick={registerPaymentId}
            disabled={!!loading || !merchantIdInput.trim()}
          >
            {loading === "payment-id" ? "Proving..." : "Create Registration"}
          </button>
        </div>

        {registrationToken && (
          <div style={{ marginBottom: "1rem" }}>
            <p style={{ color: "#f5a623", fontSize: "0.85rem", fontWeight: 600, marginBottom: "0.4rem" }}>
              Copy this token and paste it into the merchant&apos;s &quot;Register Beneficiary&quot; box:
            </p>
            <div style={{ position: "relative" }}>
              <textarea
                readOnly
                value={registrationToken}
                rows={3}
                style={{
                  width: "100%",
                  background: "#111",
                  border: "1px solid #444",
                  borderRadius: "0.5rem",
                  padding: "0.5rem 0.75rem",
                  color: "#ccc",
                  fontFamily: "monospace",
                  fontSize: "0.75rem",
                  resize: "none",
                  boxSizing: "border-box",
                }}
              />
            </div>
            <button
              className="btn-outline"
              style={{ fontSize: "0.8rem", padding: "0.25rem 0.75rem", marginTop: "0.4rem" }}
              onClick={() => { navigator.clipboard.writeText(registrationToken); toast("Token copied", "success"); }}
            >
              Copy Token
            </button>
          </div>
        )}

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
                      <span className={`badge ${r.status === "verified" ? "badge-success" : "badge-warning"}`}>{r.status}</span>
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
        locked={registrations.length === 0}
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
            {merchants.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
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
            {loading === "payment" ? "Proving..." : "Create Token"}
          </button>
        </div>

        {paymentToken && (
          <div style={{ marginBottom: "1rem" }}>
            <p style={{ color: "#f5a623", fontSize: "0.85rem", fontWeight: 600, marginBottom: "0.4rem" }}>
              Copy this payment token and paste it into the merchant&apos;s &quot;Receive Payment&quot; box:
            </p>
            <div style={{ position: "relative" }}>
              <textarea
                readOnly
                value={paymentToken}
                rows={3}
                style={{
                  width: "100%",
                  background: "#111",
                  border: "1px solid #444",
                  borderRadius: "0.5rem",
                  padding: "0.5rem 0.75rem",
                  color: "#ccc",
                  fontFamily: "monospace",
                  fontSize: "0.75rem",
                  resize: "none",
                  boxSizing: "border-box",
                }}
              />
            </div>
            <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap", marginTop: "0.4rem", alignItems: "center" }}>
              <button
                className="btn-outline"
                style={{ fontSize: "0.8rem", padding: "0.25rem 0.75rem" }}
                onClick={() => { navigator.clipboard.writeText(paymentToken); toast("Token copied", "success"); }}
              >
                Copy Token
              </button>
              <button
                className="btn-outline"
                style={{ fontSize: "0.8rem", padding: "0.25rem 0.75rem" }}
                onClick={checkPaymentAddress}
              >
                {addressSats !== null ? `${addressSats.toLocaleString()} sats received` : "Check Address"}
              </button>
              {paymentAddress && (
                <span style={{ color: "#666", fontSize: "0.8rem" }}>
                  Paying to: <code style={{ color: "#aaa" }}>{paymentAddress.slice(0, 20)}…</code>
                </span>
              )}
            </div>
          </div>
        )}

        {payments.length > 0 && (
          <div className="table-wrap">
            <table style={{ width: "100%", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                  <th style={{ padding: "0.5rem" }}>Merchant</th>
                  <th style={{ padding: "0.5rem" }}>Amount</th>
                  <th style={{ padding: "0.5rem" }}>P2TR Address</th>
                  <th style={{ padding: "0.5rem" }}>TxID</th>
                  <th style={{ padding: "0.5rem" }}>Received</th>
                </tr>
              </thead>
              <tbody>
                {payments.map((p, i) => {
                  const received = (paymentUtxos[p.address] ?? [])[0];
                  return (
                  <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                    <td style={{ padding: "0.5rem" }}>{p.merchant_name}</td>
                    <td style={{ padding: "0.5rem" }}>{p.amount} sats</td>
                    <td style={{ padding: "0.5rem" }}>
                      <HexDisplay value={p.address} />
                    </td>
                    <td style={{ padding: "0.5rem" }}>
                      {received
                        ? <HexDisplay value={received.txid} />
                        : <span style={{ color: "#555" }}>Pending…</span>}
                    </td>
                    <td style={{ padding: "0.5rem" }}>
                      {received ? `${received.amount_sats.toLocaleString()} sats` : "—"}
                    </td>
                  </tr>
                  );
                })}
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
