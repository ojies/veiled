"use client";

import { useState } from "react";
import { useToast } from "./ToastProvider";

interface FaucetButtonProps {
  walletNames: string[];
  onComplete?: () => void;
  compact?: boolean;
}

export default function FaucetButton({
  walletNames,
  onComplete,
  compact = false,
}: FaucetButtonProps) {
  const [loading, setLoading] = useState(false);
  const { toast } = useToast();

  const handleFaucet = async () => {
    if (walletNames.length === 0) {
      toast("No wallets to fund", "error");
      return;
    }

    setLoading(true);
    try {
      const res = await fetch("/api/wallet/faucet", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ names: walletNames }),
      });
      const data = await res.json();
      if (data.error) throw new Error(data.error);

      const funded = Object.keys(data.results || {}).filter(
        (k) => data.results[k].funded
      );
      toast(`Funded ${funded.length} wallet(s) via regtest mining`, "success");
      onComplete?.();
    } catch (err: any) {
      toast(`Faucet error: ${err.message}`, "error");
    } finally {
      setLoading(false);
    }
  };

  return (
    <button
      className={`faucet-btn ${compact ? "faucet-btn--compact" : ""}`}
      onClick={handleFaucet}
      disabled={loading}
    >
      {loading ? "Mining..." : compact ? "⛏" : "⛏ Fund All Wallets"}
    </button>
  );
}
