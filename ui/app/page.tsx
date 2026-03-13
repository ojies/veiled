"use client";

import { useRouter } from "next/navigation";
import { useState, useMemo } from "react";
import FaucetButton from "@/components/FaucetButton";
import { useToast } from "@/components/ToastProvider";

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
  const { toast } = useToast();

  async function handleReset() {
    try {
      await fetch("/api/reset", { method: "POST" });
      toast("Demo state reset", "success");
    } catch {
      toast("Reset failed", "error");
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
        <p style={{ color: "#666", marginBottom: "2rem", fontSize: "1.1rem" }}>
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
            onClick={() => router.push("/beneficiary")}
            style={{
              width: "100%",
              maxWidth: "320px",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <div style={{ fontSize: "1.5rem", fontWeight: 700, marginBottom: "0.75rem" }}>
              I am a Beneficiary
            </div>
            <p style={{ color: "#999", lineHeight: 1.6 }}>
              Create credentials, register with merchants, and receive Bitcoin
              payments — all while keeping your identity private.
            </p>
            <div style={{ marginTop: "1rem", color: "#f5a623", fontSize: "0.85rem" }}>
              Create Identity → Register → Connect → Receive →
            </div>
          </button>

          <button
            className="card"
            onClick={() => router.push("/merchant")}
            style={{
              width: "100%",
              maxWidth: "320px",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <div style={{ fontSize: "1.5rem", fontWeight: 700, marginBottom: "0.75rem" }}>
              I am a Merchant
            </div>
            <p style={{ color: "#999", lineHeight: 1.6 }}>
              Register your business, verify beneficiary proofs, and process
              payment requests with P2TR addresses.
            </p>
            <div style={{ marginTop: "1rem", color: "#f5a623", fontSize: "0.85rem" }}>
              Register → Fund → Verify → Send Payments →
            </div>
          </button>
        </div>

        {/* Action buttons */}
        <div style={{ display: "flex", gap: "1rem", justifyContent: "center", marginTop: "2.5rem", flexWrap: "wrap" }}>
          <FaucetButton walletNames={["registry"]} compact={false} />
          <button className="btn-outline" onClick={handleReset}>
            Reset Demo
          </button>
        </div>

        {/* Protocol overview */}
        <div
          style={{
            marginTop: "3rem",
            padding: "1.5rem",
            background: "#111",
            borderRadius: "0.75rem",
            border: "1px solid #222",
            textAlign: "left",
            maxWidth: "600px",
            marginLeft: "auto",
            marginRight: "auto",
          }}
        >
          <h3 style={{ fontWeight: 600, marginBottom: "0.75rem", fontSize: "0.95rem" }}>
            How Veiled Works
          </h3>
          <ul style={{ color: "#999", fontSize: "0.85rem", lineHeight: 1.8, paddingLeft: "1.25rem" }}>
            <li>Beneficiaries create anonymous credentials using ZK proofs</li>
            <li>A one-out-of-many proof proves set membership without revealing identity</li>
            <li>Each merchant sees a unique pseudonym — cross-merchant linking is impossible</li>
            <li>Payments use P2TR (Taproot) addresses derived from Schnorr signatures</li>
            <li>All participants have regtest Bitcoin wallets with real transactions</li>
          </ul>
        </div>
      </div>
    </>
  );
}
