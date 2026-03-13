"use client";

import { useRouter } from "next/navigation";
import { useState, useMemo } from "react";

function Particles({ count }: { count: number }) {
  const items = useMemo(() => {
    const seed = 42;
    return Array.from({ length: count }, (_, i) => {
      const s = ((seed * (i + 1) * 9301 + 49297) % 233280) / 233280;
      const s2 = ((seed * (i + 2) * 9301 + 49297) % 233280) / 233280;
      const s3 = ((seed * (i + 3) * 9301 + 49297) % 233280) / 233280;
      return {
        left: `${s * 100}%`,
        bottom: `${-s2 * 20}%`,
        duration: `${8 + s3 * 12}s`,
        delay: `${s * 8}s`,
        size: `${1.5 + s2 * 2}px`,
      };
    });
  }, [count]);

  return (
    <>
      {items.map((p, i) => (
        <div
          key={i}
          className="particle"
          style={{
            left: p.left,
            bottom: p.bottom,
            width: p.size,
            height: p.size,
            animationDuration: p.duration,
            animationDelay: p.delay,
          }}
        />
      ))}
    </>
  );
}

export default function Home() {
  const router = useRouter();
  const [setting, setSetting] = useState(false);

  async function handleSetup(role: string) {
    setSetting(true);
    try {
      const res = await fetch("/api/admin/setup", { method: "POST" });
      if (!res.ok) {
        const data = await res.json();
        if (!data.error?.includes("already")) {
          alert(`Setup failed: ${data.error}`);
          setSetting(false);
          return;
        }
      }
      router.push(`/${role}`);
    } catch {
      alert("Failed to connect to backend. Is the registry server running?");
      setSetting(false);
    }
  }

  return (
    <>
      {/* Animated background */}
      <div className="landing-bg">
        <div className="grid-overlay" />
        <div className="glow-orb" />
        <div className="ring ring-1" />
        <div className="ring ring-2" />
        <div className="ring ring-3" />
        <div className="ring ring-4" />
        <Particles count={18} />
      </div>

      <div className="landing-content" style={{ marginTop: "2rem", textAlign: "center" }}>
      <img
        src="/banner.svg"
        alt="Veiled — Verified Payments, Veiled Identities"
        style={{
          width: "100%",
          maxWidth: "720px",
          margin: "0 auto 2rem",
          borderRadius: "12px",
          border: "1px solid #222",
        }}
      />
      <p style={{ color: "#666", marginBottom: "3rem", fontSize: "1.1rem" }}>
        Choose your role to experience the protocol
      </p>

      <div
        style={{
          display: "flex",
          gap: "2rem",
          justifyContent: "center",
          flexWrap: "wrap",
        }}
      >
        <button
          className="card"
          onClick={() => handleSetup("beneficiary")}
          disabled={setting}
          style={{
            width: "100%",
            maxWidth: "320px",
            cursor: setting ? "wait" : "pointer",
            textAlign: "left",
          }}
        >
          <div
            style={{
              fontSize: "1.5rem",
              fontWeight: 700,
              marginBottom: "0.75rem",
            }}
          >
            I am a Beneficiary
          </div>
          <p style={{ color: "#999", lineHeight: 1.6 }}>
            Create credentials, register with merchants, and receive Bitcoin
            payments — all while keeping your identity private.
          </p>
          <div
            style={{
              marginTop: "1rem",
              color: "#f5a623",
              fontSize: "0.85rem",
            }}
          >
            Phases 1-5 →
          </div>
        </button>

        <button
          className="card"
          onClick={() => handleSetup("merchant")}
          disabled={setting}
          style={{
            width: "100%",
            maxWidth: "320px",
            cursor: setting ? "wait" : "pointer",
            textAlign: "left",
          }}
        >
          <div
            style={{
              fontSize: "1.5rem",
              fontWeight: 700,
              marginBottom: "0.75rem",
            }}
          >
            I am a Merchant
          </div>
          <p style={{ color: "#999", lineHeight: 1.6 }}>
            Verify beneficiary proofs, manage registered identities, and process
            payment requests with P2TR addresses.
          </p>
          <div
            style={{
              marginTop: "1rem",
              color: "#f5a623",
              fontSize: "0.85rem",
            }}
          >
            Phases 0, 4-5 →
          </div>
        </button>
      </div>

      {setting && (
        <p style={{ marginTop: "2rem", color: "#f5a623" }}>
          Initializing system…
        </p>
      )}
    </div>
    </>
  );
}
