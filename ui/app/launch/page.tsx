"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { useToast } from "@/components/ToastProvider";

interface Config {
  minMerchants: number;
  beneficiaryCapacity: number;
}

export default function LaunchPage() {
  const router = useRouter();
  const { toast } = useToast();
  const [config, setConfig] = useState<Config | null>(null);

  useEffect(() => {
    fetch("/api/config")
      .then((r) => r.json())
      .then(setConfig)
      .catch(() => setConfig({ minMerchants: 2, beneficiaryCapacity: 4 }));
  }, []);

  const minMerchants = config?.minMerchants ?? 2;
  const beneficiaryCapacity = config?.beneficiaryCapacity ?? 4;

  const [blocked, setBlocked] = useState(false);

  function handleOpenAll() {
    const total = minMerchants + beneficiaryCapacity;

    // Collect all window refs so we can close them if the browser blocks partway.
    const wins: Window[] = [];
    for (let i = 0; i < minMerchants; i++) {
      const w = window.open(`/merchant?tab=${i + 1}`, "_blank");
      if (w) wins.push(w);
      else break;
    }
    if (wins.length === minMerchants) {
      for (let i = 0; i < beneficiaryCapacity; i++) {
        const w = window.open(`/beneficiary?tab=${i + 1}`, "_blank");
        if (w) wins.push(w);
        else break;
      }
    }

    if (wins.length < total) {
      // Browser blocked partway — close the orphan tabs.
      wins.forEach((w) => w.close());
      setBlocked(true);
      toast(
        "Popups blocked — click \"Allow\" in your browser's popup notification, then try again.",
        "error"
      );
    } else {
      setBlocked(false);
      toast(`Opened ${total} tabs`, "success");
    }
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
        Launch Demo
      </h1>
      <p style={{ color: "#666", marginBottom: "2rem" }}>
        Open merchant and beneficiary tabs to run the full demo. If your browser
        blocks popups, click each link individually.
      </p>

      {blocked && (
        <div
          style={{
            background: "rgba(245, 166, 35, 0.1)",
            border: "1px solid rgba(245, 166, 35, 0.3)",
            borderRadius: "8px",
            padding: "0.75rem 1rem",
            marginBottom: "1.5rem",
            color: "#f5a623",
            fontSize: "0.85rem",
          }}
        >
          Your browser blocked some popups. Allow popups for this site in your
          browser settings and try again, or open each tab individually below.
        </div>
      )}

      <div style={{ display: "flex", gap: "1rem", marginBottom: "2rem", flexWrap: "wrap" }}>
        <button className="btn" onClick={handleOpenAll} style={{ fontSize: "1rem", padding: "0.65rem 2.5rem" }}>
          Launch All ({minMerchants + beneficiaryCapacity})
        </button>
        <button
          className="btn-outline"
          onClick={() => router.push("/demo")}
          style={{ fontSize: "1rem", padding: "0.65rem 2rem" }}
        >
          Demo Controls
        </button>
      </div>

      {/* Merchants */}
      <h2 style={{ fontSize: "1.1rem", fontWeight: 600, marginBottom: "0.75rem" }}>
        Merchants ({minMerchants})
      </h2>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))",
          gap: "0.75rem",
          marginBottom: "2rem",
        }}
      >
        {Array.from({ length: minMerchants }, (_, i) => (
          <a
            key={`m-${i}`}
            href={`/merchant?tab=${i + 1}`}
            target="_blank"
            rel="noopener noreferrer"
            className="card"
            style={{
              display: "flex",
              alignItems: "center",
              gap: "0.5rem",
              textDecoration: "none",
              color: "inherit",
              cursor: "pointer",
            }}
          >
            <span style={{ fontSize: "1.4rem" }}>&#128722;</span>
            <span style={{ fontWeight: 600 }}>Merchant {i + 1}</span>
          </a>
        ))}
      </div>

      {/* Beneficiaries */}
      <h2 style={{ fontSize: "1.1rem", fontWeight: 600, marginBottom: "0.75rem" }}>
        Beneficiaries ({beneficiaryCapacity})
      </h2>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))",
          gap: "0.75rem",
        }}
      >
        {Array.from({ length: beneficiaryCapacity }, (_, i) => (
          <a
            key={`b-${i}`}
            href={`/beneficiary?tab=${i + 1}`}
            target="_blank"
            rel="noopener noreferrer"
            className="card"
            style={{
              display: "flex",
              alignItems: "center",
              gap: "0.5rem",
              textDecoration: "none",
              color: "inherit",
              cursor: "pointer",
            }}
          >
            <span style={{ fontSize: "1.4rem" }}>&#128274;</span>
            <span style={{ fontWeight: 600 }}>Beneficiary {i + 1}</span>
          </a>
        ))}
      </div>
    </div>
  );
}
