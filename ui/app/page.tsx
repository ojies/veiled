"use client";

import { useRouter } from "next/navigation";
import { useMemo } from "react";

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

const PROTOCOL_STEPS = [
  {
    title: "Credential Creation",
    desc: "Beneficiary generates secrets locally and computes a Pedersen commitment packing per-merchant nullifiers into a single 33-byte curve point.",
  },
  {
    title: "Anonymous Registration",
    desc: "The commitment is registered in a fixed-size anonymity set, sealed on Bitcoin via a VTxO tree of P2TR outputs.",
  },
  {
    title: "ZK Proof of Membership",
    desc: "A Bootle/Groth one-out-of-many proof proves the beneficiary is in the anonymity set without revealing which member.",
  },
  {
    title: "Unlinkable Pseudonyms",
    desc: "Each merchant receives a unique pseudonym derived via HKDF. Two colluding merchants cannot link their pseudonyms cryptographically.",
  },
  {
    title: "Sybil Resistance",
    desc: "Deterministic nullifiers prevent double-registration. Same master secret + same merchant always produces the same nullifier.",
  },
  {
    title: "Schnorr Authentication",
    desc: "After initial registration, beneficiaries authenticate with a lightweight 65-byte Schnorr proof. No further ZK proofs needed.",
  },
];

export default function Home() {
  const router = useRouter();

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
            margin: "0 auto 1.5rem",
            borderRadius: "12px",
            border: "1px solid #222",
          }}
        />
        <h2
          className="gradient-text"
          style={{
            fontSize: "clamp(1.1rem, 3vw, 1.4rem)",
            fontWeight: 600,
            marginBottom: "0.5rem",
          }}
        >
          Anonymous Self-Credentials on Bitcoin
        </h2>
        <p style={{ color: "#666", marginBottom: "2.5rem", fontSize: "1rem" }}>
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
            className="card role-card"
            onClick={() => router.push("/beneficiary")}
            style={{
              width: "100%",
              maxWidth: "340px",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: "0.65rem", marginBottom: "0.75rem" }}>
              <span style={{ fontSize: "1.8rem" }}>&#128274;</span>
              <span style={{ fontSize: "1.5rem", fontWeight: 700 }}>I am a Beneficiary</span>
            </div>
            <p style={{ color: "#999", lineHeight: 1.6 }}>
              Create credentials, register with merchants, and receive Bitcoin
              payments — all while keeping your identity private.
            </p>
            <div style={{ marginTop: "1rem", color: "#f5a623", fontSize: "0.85rem", fontWeight: 500 }}>
              Create Identity &rarr; Register &rarr; Connect &rarr; Receive
            </div>
          </button>

          <button
            className="card role-card"
            onClick={() => router.push("/merchant")}
            style={{
              width: "100%",
              maxWidth: "340px",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: "0.65rem", marginBottom: "0.75rem" }}>
              <span style={{ fontSize: "1.8rem" }}>&#128722;</span>
              <span style={{ fontSize: "1.5rem", fontWeight: 700 }}>I am a Merchant</span>
            </div>
            <p style={{ color: "#999", lineHeight: 1.6 }}>
              Register your business, verify beneficiary proofs, and process
              payment requests with P2TR addresses.
            </p>
            <div style={{ marginTop: "1rem", color: "#f5a623", fontSize: "0.85rem", fontWeight: 500 }}>
              Register &rarr; Fund &rarr; Verify &rarr; Send Payments
            </div>
          </button>
        </div>

        {/* Launch Demo button */}
        <div style={{ marginTop: "2rem" }}>
          <button
            className="btn"
            onClick={() => router.push("/demo")}
            style={{ fontSize: "1rem", padding: "0.65rem 2.5rem" }}
          >
            Launch Demo
          </button>
        </div>
      </div>

      {/* How Veiled Works — full-width section */}
      <section
        style={{
          marginTop: "4rem",
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          background: "linear-gradient(180deg, transparent, #0d0d0d 40%)",
        }}
      >
        <h2
          style={{
            textAlign: "center",
            fontSize: "clamp(1.2rem, 4vw, 1.6rem)",
            fontWeight: 700,
            marginBottom: "0.5rem",
          }}
        >
          How Veiled Works
        </h2>
        <p
          style={{
            textAlign: "center",
            color: "#666",
            marginBottom: "2.5rem",
            fontSize: "0.9rem",
          }}
        >
          Anonymous Self-Credentials on Bitcoin using Bootle/Groth ZK proofs
        </p>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
            gap: "1.25rem",
            maxWidth: "900px",
            margin: "0 auto",
          }}
        >
          {PROTOCOL_STEPS.map((step, i) => (
            <div key={i} className="protocol-step">
              <div
                style={{
                  color: "#f5a623",
                  fontSize: "0.75rem",
                  fontWeight: 600,
                  marginBottom: "0.5rem",
                  letterSpacing: "0.05em",
                  textTransform: "uppercase",
                  display: "flex",
                  alignItems: "center",
                  gap: "0.4rem",
                }}
              >
                <span style={{
                  width: "20px",
                  height: "20px",
                  borderRadius: "50%",
                  border: "1.5px solid rgba(245, 166, 35, 0.4)",
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: "0.65rem",
                }}>
                  {i + 1}
                </span>
                Step {i + 1}
              </div>
              <h3 style={{ fontWeight: 600, marginBottom: "0.4rem", fontSize: "0.95rem" }}>
                {step.title}
              </h3>
              <p style={{ color: "#888", fontSize: "0.83rem", lineHeight: 1.6 }}>
                {step.desc}
              </p>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
