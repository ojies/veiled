"use client";

import { useState } from "react";
import HexDisplay from "./HexDisplay";

interface WalletCardProps {
  name: string;
  address: string;
  balance: number;
  mnemonic?: string;
  onFaucet?: () => void;
  onSend?: (toAddress: string, amount: number) => void;
  loading?: boolean;
}

export default function WalletCard({
  name,
  address,
  balance,
  mnemonic,
  onFaucet,
  onSend,
  loading = false,
}: WalletCardProps) {
  const [showMnemonic, setShowMnemonic] = useState(false);
  const [showSend, setShowSend] = useState(false);
  const [sendTo, setSendTo] = useState("");
  const [sendAmount, setSendAmount] = useState("");

  const formatSats = (sats: number) => {
    if (sats >= 100_000_000) return `${(sats / 100_000_000).toFixed(4)} BTC`;
    return `${sats.toLocaleString()} sats`;
  };

  return (
    <div className="wallet-card">
      <div className="wallet-header">
        <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#F5A623" strokeWidth="2">
            <rect x="2" y="6" width="20" height="14" rx="2" />
            <path d="M2 10h20" />
            <circle cx="18" cy="14" r="1.5" fill="#F5A623" />
          </svg>
          <span style={{ fontWeight: 600 }}>{name}</span>
        </div>
        <span className="wallet-balance">{formatSats(balance)}</span>
      </div>

      <div className="wallet-address">
        <span style={{ color: "#888", fontSize: "0.75rem" }}>Address</span>
        <HexDisplay value={address} full />
      </div>

      {mnemonic && (
        <div style={{ marginTop: "0.5rem" }}>
          <button
            onClick={() => setShowMnemonic(!showMnemonic)}
            style={{
              background: "none",
              border: "none",
              color: "#888",
              cursor: "pointer",
              fontSize: "0.75rem",
              padding: 0,
              textDecoration: "underline",
            }}
          >
            {showMnemonic ? "Hide" : "Show"} recovery phrase
          </button>
          {showMnemonic && (
            <div
              style={{
                marginTop: "0.25rem",
                padding: "0.5rem",
                background: "#1a1a1a",
                borderRadius: "4px",
                fontSize: "0.8rem",
                fontFamily: "var(--font-geist-mono)",
                wordSpacing: "0.3em",
                lineHeight: 1.6,
              }}
            >
              {mnemonic}
            </div>
          )}
        </div>
      )}

      <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.75rem" }}>
        {onFaucet && (
          <button
            className="faucet-btn"
            onClick={onFaucet}
            disabled={loading}
          >
            {loading ? "Mining..." : "⛏ Fund Wallet"}
          </button>
        )}
        {onSend && (
          <button
            style={{
              background: "#222",
              border: "1px solid #333",
              color: "#eee",
              padding: "0.4rem 0.8rem",
              borderRadius: "6px",
              cursor: "pointer",
              fontSize: "0.8rem",
            }}
            onClick={() => setShowSend(!showSend)}
          >
            Send
          </button>
        )}
      </div>

      {showSend && onSend && (
        <div
          style={{
            marginTop: "0.5rem",
            display: "flex",
            gap: "0.5rem",
            flexWrap: "wrap",
          }}
        >
          <input
            placeholder="Address"
            value={sendTo}
            onChange={(e) => setSendTo(e.target.value)}
            style={{
              flex: 1,
              minWidth: "200px",
              background: "#111",
              border: "1px solid #333",
              color: "#eee",
              padding: "0.4rem",
              borderRadius: "4px",
              fontSize: "0.8rem",
              fontFamily: "var(--font-geist-mono)",
            }}
          />
          <input
            placeholder="Sats"
            type="number"
            value={sendAmount}
            onChange={(e) => setSendAmount(e.target.value)}
            style={{
              width: "100px",
              background: "#111",
              border: "1px solid #333",
              color: "#eee",
              padding: "0.4rem",
              borderRadius: "4px",
              fontSize: "0.8rem",
            }}
          />
          <button
            onClick={() => {
              onSend(sendTo, parseInt(sendAmount) || 0);
              setSendTo("");
              setSendAmount("");
              setShowSend(false);
            }}
            style={{
              background: "#F5A623",
              border: "none",
              color: "#000",
              padding: "0.4rem 0.8rem",
              borderRadius: "6px",
              cursor: "pointer",
              fontWeight: 600,
              fontSize: "0.8rem",
            }}
          >
            Confirm
          </button>
        </div>
      )}
    </div>
  );
}
