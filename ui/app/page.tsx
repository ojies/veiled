"use client";

import { useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";
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

const PRIVACY_PROPERTIES = [
  {
    icon: "\u{1F6E1}",
    title: "Unlinkable Pseudonyms",
    desc: "Each merchant receives a cryptographically unique pseudonym. Two colluding merchants cannot link their views of the same beneficiary.",
  },
  {
    icon: "\u{1F6AB}",
    title: "Sybil Resistance",
    desc: "Deterministic nullifiers prevent double-registration. Same credential + same merchant always yields the same nullifier.",
  },
  {
    icon: "\u{1F50D}",
    title: "Zero-Knowledge Proofs",
    desc: "Bootle/Groth one-out-of-many proofs verify set membership without revealing which commitment is yours.",
  },
  {
    icon: "\u26D3",
    title: "Bitcoin Anchored",
    desc: "Anonymity sets are sealed on-chain via Taproot commitments with P2TR outputs, providing a tamper-proof public record.",
  },
];

const ARCHITECTURE_LAYERS = [
  {
    label: "Frontend",
    items: ["Next.js Web UI", "Role-based flows", "Real-time wallet sync"],
    color: "#60a5fa",
  },
  {
    label: "API Layer",
    items: ["REST routes", "gRPC bridge", "JSON stdin/stdout binaries"],
    color: "#f5a623",
  },
  {
    label: "Crypto & Wallet",
    items: ["veiled-core (ZK proofs)", "veiled-wallet (BDK/BIP86)", "Schnorr signatures"],
    color: "#4ade80",
  },
  {
    label: "Bitcoin",
    items: ["bitcoind regtest", "P2TR addresses", "Taproot commitment"],
    color: "#f87171",
  },
];

const TECH_STACK = [
  { name: "Rust", desc: "Core cryptography, wallet binary, gRPC services" },
  { name: "BDK", desc: "bdk_wallet v2 for BIP86 P2TR descriptor wallets" },
  { name: "secp256k1", desc: "Elliptic curve operations, Pedersen commitments" },
  { name: "Bitcoin Core", desc: "Regtest node for chain interaction via RPC" },
  { name: "tonic/prost", desc: "gRPC server and client implementation" },
  { name: "Next.js", desc: "React-based UI with server-side API routes" },
];

const PROTOCOL_STEPS = [
  {
    title: "Credential Creation",
    desc: "Beneficiary generates secrets locally and computes a Pedersen commitment packing per-merchant nullifiers into a single 33-byte curve point.",
  },
  {
    title: "Anonymous Registration",
    desc: "The commitment is registered in a fixed-size anonymity set, sealed on Bitcoin via a Taproot commitment with P2TR outputs.",
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
  const { toast } = useToast();
  const [launching, setLaunching] = useState(false);
  const [config, setConfig] = useState<{ minMerchants: number; beneficiaryCapacity: number } | null>(null);

  useEffect(() => {
    fetch("/api/config")
      .then((r) => r.json())
      .then(setConfig)
      .catch(() => {});
  }, []);

  function handleLaunchDemo() {
    const minMerchants = config?.minMerchants ?? 2;
    const beneficiaryCapacity = config?.beneficiaryCapacity ?? 4;
    const total = minMerchants + beneficiaryCapacity;

    setLaunching(true);
    toast(`Opening ${total} tabs (${minMerchants} merchant + ${beneficiaryCapacity} beneficiary)...`, "info");

    // Open all tabs synchronously within the click handler so the browser
    // does not treat them as blocked popups (must be in the user-gesture
    // call stack, before any await).
    for (let i = 0; i < minMerchants; i++) {
      window.open(`/merchant?tab=${i + 1}`, "_blank");
    }
    for (let i = 0; i < beneficiaryCapacity; i++) {
      window.open(`/beneficiary?tab=${i + 1}`, "_blank");
    }

    // Fund registry wallet in the background (non-blocking).
    fetch("/api/wallet/faucet", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ names: ["registry"] }),
    })
      .then(() => toast(`Launched ${total} tabs`, "success"))
      .catch((e: any) => toast(e.message || "Faucet failed", "error"))
      .finally(() => setLaunching(false));
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

        {/* Launch Demo */}
        <div style={{ marginTop: "2rem", display: "flex", gap: "1rem", justifyContent: "center", flexWrap: "wrap" }}>
          <button
            className="btn"
            onClick={handleLaunchDemo}
            disabled={launching}
            style={{ fontSize: "1rem", padding: "0.65rem 2.5rem" }}
          >
            {launching ? "Launching..." : "Launch Demo"}
          </button>
          <button
            className="btn-outline"
            onClick={() => router.push("/demo")}
            style={{ fontSize: "1rem", padding: "0.65rem 2rem" }}
          >
            Demo Controls
          </button>
        </div>
      </div>

      {/* About */}
      <section
        id="about"
        style={{
          marginTop: "4rem",
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          scrollMarginTop: "60px",
          maxWidth: "800px",
          margin: "4rem auto 0",
        }}
      >
        <h2
          style={{
            textAlign: "center",
            fontSize: "clamp(1.2rem, 4vw, 1.6rem)",
            fontWeight: 700,
            marginBottom: "1.5rem",
          }}
        >
          About Veiled
        </h2>
        <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem", marginBottom: "1rem" }}>
          Veiled is a pseudonymous payment verification system on Bitcoin implementing
          the <strong style={{ color: "#ededed" }}>Anonymous Self-Credentials (ASC)</strong> protocol
          by Alupotha et al. It allows beneficiaries to receive payments from multiple
          merchants while keeping their true identity private.
        </p>
        <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem", marginBottom: "1rem" }}>
          A beneficiary registers a single master credential once, derives an unlinkable
          payment identity for each merchant using hash-based key derivation, and proves
          ownership through a zero-knowledge proof without revealing which credential is
          theirs among a public anonymity set. Each merchant receives a unique nullifier
          that prevents Sybil attacks, while pseudonyms remain cryptographically unlinkable
          across merchants.
        </p>
        <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem" }}>
          Once registered, beneficiaries authenticate via lightweight Schnorr signatures
          and receive Bitcoin payments to P2TR addresses derived from their pseudonyms —
          no further interaction with the registry required.
        </p>
      </section>

      {/* How It Works */}
      <section
        id="how-it-works"
        style={{
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          background: "linear-gradient(180deg, transparent, #0d0d0d 40%)",
          scrollMarginTop: "60px",
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
          How It Works
        </h2>
        <p
          style={{
            textAlign: "center",
            color: "#666",
            marginBottom: "2.5rem",
            fontSize: "0.9rem",
          }}
        >
          Six steps from credential creation to authenticated payment
        </p>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
            gap: "1.25rem",
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

      {/* Privacy Properties */}
      <section
        id="privacy"
        style={{
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          scrollMarginTop: "60px",
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
          Privacy &amp; Security Properties
        </h2>
        <p
          style={{
            textAlign: "center",
            color: "#666",
            marginBottom: "2.5rem",
            fontSize: "0.9rem",
          }}
        >
          Cryptographic guarantees baked into every layer of the protocol
        </p>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))",
            gap: "1.25rem",
          }}
        >
          {PRIVACY_PROPERTIES.map((prop, i) => (
            <div key={i} className="privacy-card">
              <div style={{ fontSize: "1.6rem", marginBottom: "0.75rem" }}>
                {prop.icon}
              </div>
              <h3 style={{ fontWeight: 600, marginBottom: "0.4rem", fontSize: "0.95rem" }}>
                {prop.title}
              </h3>
              <p style={{ color: "#888", fontSize: "0.83rem", lineHeight: 1.6 }}>
                {prop.desc}
              </p>
            </div>
          ))}
        </div>
      </section>

      {/* Architecture Overview */}
      <section
        id="architecture"
        style={{
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          background: "linear-gradient(180deg, transparent, #0d0d0d 40%)",
          scrollMarginTop: "60px",
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
          Architecture
        </h2>
        <p
          style={{
            textAlign: "center",
            color: "#666",
            marginBottom: "2.5rem",
            fontSize: "0.9rem",
          }}
        >
          Four layers from UI to blockchain
        </p>

        <div className="arch-stack">
          {ARCHITECTURE_LAYERS.map((layer, i) => (
            <div key={i} className="arch-layer" style={{ "--layer-color": layer.color } as React.CSSProperties}>
              <div className="arch-label">{layer.label}</div>
              <div className="arch-items">
                {layer.items.map((item, j) => (
                  <span key={j} className="arch-item">{item}</span>
                ))}
              </div>
              {i < ARCHITECTURE_LAYERS.length - 1 && (
                <div className="arch-connector">
                  <svg width="16" height="24" viewBox="0 0 16 24" fill="none">
                    <path d="M8 0 L8 18 M3 14 L8 20 L13 14" stroke="#333" strokeWidth="1.5" />
                  </svg>
                </div>
              )}
            </div>
          ))}
        </div>
      </section>

      {/* Built With */}
      <section
        id="tech"
        style={{
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          background: "linear-gradient(180deg, transparent, #0d0d0d 40%)",
          scrollMarginTop: "60px",
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
          Built With
        </h2>
        <p
          style={{
            textAlign: "center",
            color: "#666",
            marginBottom: "2.5rem",
            fontSize: "0.9rem",
          }}
        >
          Production-grade open-source tooling
        </p>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))",
            gap: "1rem",
          }}
        >
          {TECH_STACK.map((tech, i) => (
            <div key={i} className="tech-card">
              <span className="tech-name">{tech.name}</span>
              <span className="tech-desc">{tech.desc}</span>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
