"use client";

import { useState, useEffect, useCallback } from "react";
import Stepper from "@/components/Stepper";
import PhaseCard from "@/components/PhaseCard";
import WalletCard from "@/components/WalletCard";
import HexDisplay from "@/components/HexDisplay";
import { useToast } from "@/components/ToastProvider";
import { useLocalState } from "@/lib/useLocalState";

interface Identity {
  beneficiary: string;
  pseudonym: string;
  nullifier: string;
}

interface PaymentRow {
  beneficiary: string;
  amount: number;
  address: string;
}

interface ExistingMerchant {
  name: string;
  port: number;
  status: string;
}

const STEPS = [
  { label: "Create Wallet" },
  { label: "Register" },
  { label: "Dashboard" },
];

export default function MerchantPage() {
  const { toast } = useToast();

  // Persisted state (survives page refresh)
  const [merchantName, setMerchantName] = useLocalState("merch:name", "");
  const [merchantOrigin, setMerchantOrigin] = useLocalState("merch:origin", "");
  const [registered, setRegistered] = useLocalState("merch:registered", false);
  const [serverPort, setServerPort] = useLocalState("merch:port", 0);
  const [walletAddress, setWalletAddress] = useLocalState("merch:walletAddr", "");
  const [walletMnemonic, setWalletMnemonic] = useLocalState("merch:walletMnemonic", "");
  const [walletName, setWalletName] = useLocalState("merch:walletName", "");
  const [walletCreated, setWalletCreated] = useLocalState("merch:walletCreated", false);

  // Ephemeral state
  const [serverStatus, setServerStatus] = useState("");
  const [regLoading, setRegLoading] = useState(false);
  const [walletBalance, setWalletBalance] = useState(0);
  const [walletLoading, setWalletLoading] = useState(false);
  const [fees, setFees] = useState<{ beneficiary: number; merchant: number } | null>(null);

  // Existing merchants (for switching)
  const [existingMerchants, setExistingMerchants] = useState<ExistingMerchant[]>([]);

  // Dashboard
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [payments, setPayments] = useState<PaymentRow[]>([]);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [sendingTo, setSendingTo] = useState<string | null>(null);

  const activeStep = !walletCreated ? 0 : !registered ? 1 : 2;

  // Fetch fees and existing merchants on mount
  useEffect(() => {
    fetch("/api/setup/init", { method: "POST" })
      .then((r) => r.json())
      .then((data) => {
        if (data.fees) setFees(data.fees);
      })
      .catch(() => {});
    fetchExistingMerchants();
  }, []);

  async function fetchExistingMerchants() {
    try {
      const res = await fetch("/api/state");
      const data = await res.json();
      const procs = data.merchant_processes || {};
      const list: ExistingMerchant[] = Object.values(procs).map((p: any) => ({
        name: p.name,
        port: p.port,
        status: p.status,
      }));
      setExistingMerchants(list);
    } catch {
      // ignore
    }
  }

  function switchToMerchant(m: ExistingMerchant) {
    const wName = `merchant-${m.name.toLowerCase().replace(/\s+/g, "-")}`;
    setMerchantName(m.name);
    setMerchantOrigin("");
    setRegistered(true);
    setServerPort(m.port);
    setServerStatus(m.status);
    setWalletName(wName);
    setWalletCreated(true);
    setWalletAddress("");
    setWalletMnemonic("");
    toast(`Switched to merchant "${m.name}"`, "success");
  }

  function switchToNewMerchant() {
    setMerchantName("");
    setMerchantOrigin("");
    setRegistered(false);
    setServerPort(0);
    setServerStatus("");
    setWalletName("");
    setWalletCreated(false);
    setWalletAddress("");
    setWalletMnemonic("");
    setIdentities([]);
    setPayments([]);
    fetchExistingMerchants();
  }

  // Create wallet on name entry
  async function createWallet() {
    if (!merchantName.trim()) return;
    const wName = `merchant-${merchantName.toLowerCase().replace(/\s+/g, "-")}`;
    setWalletLoading(true);
    try {
      const res = await fetch("/api/wallet/create", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: wName, role: "merchant" }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      setWalletAddress(data.address);
      setWalletMnemonic(data.mnemonic || "");
      setWalletName(wName);
      setWalletCreated(true);
      toast("Wallet created", "success");
    } catch (e: any) {
      toast(e.message, "error");
    }
    setWalletLoading(false);
  }

  // Fund wallet
  async function fundWallet() {
    setWalletLoading(true);
    try {
      const res = await fetch("/api/wallet/faucet", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ names: [walletName] }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      toast("Wallet funded via regtest mining", "success");
      await refreshBalance();
    } catch (e: any) {
      toast(e.message, "error");
    }
    setWalletLoading(false);
  }

  // Refresh balance
  const refreshBalance = useCallback(async () => {
    if (!walletName) return;
    try {
      const res = await fetch(`/api/wallet/balance?name=${encodeURIComponent(walletName)}`);
      const data = await res.json();
      setWalletBalance(data.total || 0);
    } catch {
      // ignore
    }
  }, [walletName]);

  useEffect(() => {
    if (walletCreated) {
      refreshBalance();
      const interval = setInterval(refreshBalance, 5000);
      return () => clearInterval(interval);
    }
  }, [walletCreated, refreshBalance]);

  // Register merchant (spawn gRPC server)
  async function registerMerchant() {
    if (!merchantName.trim() || !merchantOrigin.trim()) return;
    if (fees && walletBalance < fees.merchant) {
      toast(
        `Insufficient balance: need ${fees.merchant.toLocaleString()} sats, have ${walletBalance.toLocaleString()}. Fund your wallet first.`,
        "error"
      );
      return;
    }
    setRegLoading(true);
    try {
      const res = await fetch("/api/merchant/create", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: merchantName.trim(),
          origin: merchantOrigin.trim(),
        }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      setRegistered(true);
      setServerStatus(data.status);
      setServerPort(data.port);
      toast(`Merchant "${merchantName}" registered and server started`, "success");
      await refreshBalance();
      fetchExistingMerchants();
    } catch (e: any) {
      const msg = e.message || "Registration failed";
      if (msg.includes("already")) {
        toast("Merchant already registered", "error");
      } else if (msg.includes("insufficient") || msg.includes("amount")) {
        toast("Insufficient funds for registration fee. Use faucet to top up.", "error");
      } else if (msg.includes("UNAVAILABLE") || msg.includes("connect")) {
        toast("Cannot reach registry server", "error");
      } else {
        toast(msg, "error");
      }
    }
    setRegLoading(false);
  }

  // Dashboard refresh
  const refresh = useCallback(async () => {
    if (!registered || !merchantName) return;
    try {
      const [idRes, payRes] = await Promise.all([
        fetch(`/api/merchant/identities?merchant=${encodeURIComponent(merchantName)}`),
        fetch(`/api/merchant/payments?merchant=${encodeURIComponent(merchantName)}`),
      ]);
      const idData = await idRes.json();
      const payData = await payRes.json();
      setIdentities(idData.identities || []);
      setPayments(payData.payments || []);
    } catch {
      // ignore
    }
  }, [registered, merchantName]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!autoRefresh || !registered) return;
    const interval = setInterval(refresh, 3000);
    return () => clearInterval(interval);
  }, [autoRefresh, registered, refresh]);

  // Send payment to beneficiary
  async function sendPayment(address: string, amount: number) {
    setSendingTo(address);
    try {
      const res = await fetch("/api/wallet/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          from: walletName,
          to_address: address,
          amount_sats: amount,
        }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      toast(`Sent ${amount} sats to ${address.slice(0, 12)}...`, "success");
      // Mine a block to confirm
      await fetch("/api/wallet/faucet", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ names: ["faucet-miner"] }),
      });
      await refreshBalance();
    } catch (e: any) {
      toast(e.message, "error");
    }
    setSendingTo(null);
  }

  const totalPayments = payments.reduce((sum, p) => sum + p.amount, 0);
  // Other merchants the user can switch to (exclude the current one)
  const otherMerchants = existingMerchants.filter((m) => m.name !== merchantName);

  return (
    <div className="fade-in">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: "0.5rem", marginBottom: "0.5rem" }}>
        <h1 style={{ fontSize: "clamp(1.3rem, 5vw, 1.8rem)", fontWeight: 700 }}>
          Merchant Dashboard
          {registered && merchantName && (
            <span style={{ color: "#f5a623", fontSize: "0.7em", marginLeft: "0.5rem" }}>
              {merchantName}
            </span>
          )}
        </h1>
        {registered && (
          <button
            className="btn-outline"
            onClick={switchToNewMerchant}
            style={{ fontSize: "0.8rem", padding: "0.35rem 0.75rem" }}
          >
            Switch Merchant
          </button>
        )}
      </div>
      <p style={{ color: "#666", marginBottom: "1.5rem" }}>
        Register your business, verify proofs, and process payments
      </p>

      {/* Existing merchants — show when not registered yet */}
      {!registered && existingMerchants.length > 0 && (
        <div
          className="card"
          style={{ marginBottom: "1.5rem", padding: "1rem 1.25rem" }}
        >
          <h3 style={{ fontSize: "0.9rem", fontWeight: 600, marginBottom: "0.75rem" }}>
            Existing Merchants
          </h3>
          <p style={{ color: "#888", fontSize: "0.8rem", marginBottom: "0.75rem" }}>
            Resume as an existing merchant or create a new one below.
          </p>
          <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
            {existingMerchants.map((m) => (
              <button
                key={m.name}
                className="btn-outline"
                onClick={() => switchToMerchant(m)}
                style={{ fontSize: "0.8rem", padding: "0.4rem 0.75rem" }}
              >
                {m.name}
                <span
                  style={{
                    marginLeft: "0.4rem",
                    fontSize: "0.7rem",
                    color: m.status === "running" ? "#4ade80" : "#f87171",
                  }}
                >
                  {m.status}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}

      <Stepper steps={STEPS} activeStep={activeStep} />

      {/* Create Wallet */}
      <PhaseCard
        title="Create Wallet"
        active={activeStep === 0}
        completed={walletCreated}
        defaultOpen={!walletCreated}
      >
        <div className="form-row">
          <input
            type="text"
            placeholder="Merchant name (e.g., CoffeeCo)"
            value={merchantName}
            onChange={(e) => setMerchantName(e.target.value)}
            style={{
              background: "#111",
              border: "1px solid #333",
              borderRadius: "0.5rem",
              padding: "0.5rem 0.75rem",
              color: "#fff",
              flex: 1,
              minWidth: 0,
            }}
            disabled={walletCreated}
          />
          <button
            className="btn"
            onClick={createWallet}
            disabled={walletLoading || !merchantName.trim() || walletCreated}
          >
            {walletLoading ? "Creating..." : walletCreated ? "Created" : "Create Wallet"}
          </button>
        </div>
      </PhaseCard>

      {/* Wallet Card */}
      {walletCreated && (
        <WalletCard
          name={`${merchantName}'s Wallet`}
          address={walletAddress}
          balance={walletBalance}
          mnemonic={walletMnemonic}
          onFaucet={fundWallet}
          loading={walletLoading}
        />
      )}

      {/* Register */}
      <PhaseCard
        title="Register Merchant"
        active={activeStep === 1}
        completed={registered}
        locked={!walletCreated}
      >
        <div className="form-row" style={{ marginBottom: "0.75rem" }}>
          <input
            type="text"
            placeholder="Origin URL (e.g., https://coffeeco.com)"
            value={merchantOrigin}
            onChange={(e) => setMerchantOrigin(e.target.value)}
            style={{
              background: "#111",
              border: "1px solid #333",
              borderRadius: "0.5rem",
              padding: "0.5rem 0.75rem",
              color: "#fff",
              flex: 1,
              minWidth: 0,
            }}
            disabled={registered}
          />
          <button
            className="btn"
            onClick={registerMerchant}
            disabled={regLoading || registered || !merchantOrigin.trim()}
          >
            {regLoading ? "Starting..." : registered ? "Registered" : "Pay & Register"}
          </button>
        </div>
        <p style={{ color: "#888", fontSize: "0.8rem" }}>
          Registration fee: {fees ? fees.merchant.toLocaleString() : "..."} sats. This spawns a gRPC merchant server.
        </p>
        {registered && (
          <div style={{ marginTop: "0.75rem" }}>
            <span className="badge badge-success">
              Server {serverStatus} on port {serverPort}
            </span>
          </div>
        )}
      </PhaseCard>

      {/* Dashboard */}
      {registered && (
        <>
          {/* Stats */}
          <div className="stats-row">
            <div className="stat-card">
              <div className="stat-value">{identities.length}</div>
              <div className="stat-label">Registered Beneficiaries</div>
            </div>
            <div className="stat-card">
              <div className="stat-value">{payments.length}</div>
              <div className="stat-label">Payments</div>
            </div>
            <div className="stat-card">
              <div className="stat-value">{totalPayments.toLocaleString()}</div>
              <div className="stat-label">Total Sats</div>
            </div>
          </div>

          {/* Refresh controls */}
          <div className="form-row" style={{ marginBottom: "1rem" }}>
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

          {/* Switch to other merchants inline */}
          {otherMerchants.length > 0 && (
            <div style={{ marginBottom: "1rem", display: "flex", gap: "0.5rem", alignItems: "center", flexWrap: "wrap" }}>
              <span style={{ color: "#666", fontSize: "0.8rem" }}>Switch to:</span>
              {otherMerchants.map((m) => (
                <button
                  key={m.name}
                  className="btn-outline"
                  onClick={() => switchToMerchant(m)}
                  style={{ fontSize: "0.75rem", padding: "0.25rem 0.5rem" }}
                >
                  {m.name}
                </button>
              ))}
            </div>
          )}

          {/* Registered Beneficiaries */}
          <PhaseCard title="Registered Beneficiaries" defaultOpen active>
            {identities.length === 0 ? (
              <p style={{ color: "#666" }}>
                No beneficiaries registered yet. Waiting for incoming registrations...
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
                          <HexDisplay value={id.pseudonym} />
                        </td>
                        <td style={{ padding: "0.5rem" }}>
                          <HexDisplay value={id.nullifier} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </PhaseCard>

          {/* Payments */}
          <PhaseCard title="Payments" defaultOpen active>
            {payments.length === 0 ? (
              <p style={{ color: "#666" }}>
                No payment requests yet. Waiting for beneficiary requests...
              </p>
            ) : (
              <div className="table-wrap">
                <table style={{ width: "100%", borderCollapse: "collapse" }}>
                  <thead>
                    <tr style={{ borderBottom: "1px solid #333", textAlign: "left" }}>
                      <th style={{ padding: "0.5rem" }}>Beneficiary</th>
                      <th style={{ padding: "0.5rem" }}>Amount</th>
                      <th style={{ padding: "0.5rem" }}>P2TR Address</th>
                      <th style={{ padding: "0.5rem" }}>Action</th>
                    </tr>
                  </thead>
                  <tbody>
                    {payments.map((p, i) => (
                      <tr key={i} style={{ borderBottom: "1px solid #222" }}>
                        <td style={{ padding: "0.5rem" }}>{p.beneficiary}</td>
                        <td style={{ padding: "0.5rem" }}>{p.amount} sats</td>
                        <td style={{ padding: "0.5rem" }}>
                          <HexDisplay value={p.address} />
                        </td>
                        <td style={{ padding: "0.5rem" }}>
                          <button
                            className="faucet-btn faucet-btn--compact"
                            onClick={() => sendPayment(p.address, p.amount)}
                            disabled={sendingTo === p.address}
                          >
                            {sendingTo === p.address ? "Sending..." : "Send BTC"}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </PhaseCard>

          {/* Privacy note */}
          <PhaseCard title="What you can see as a merchant">
            <ul style={{ color: "#999", fontSize: "0.85rem", lineHeight: 1.8, paddingLeft: "1.25rem" }}>
              <li>Pseudonym — unique to your merchant, cannot be linked to other merchants</li>
              <li>Nullifier — prevents double-registration (Sybil resistance)</li>
              <li>Friendly name — revealed by the beneficiary (privacy trade-off)</li>
              <li>ZK proof verified — you know they&apos;re in the anonymity set, but not which position</li>
            </ul>
          </PhaseCard>
        </>
      )}
    </div>
  );
}
