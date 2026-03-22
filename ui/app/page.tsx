"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState } from "react";
import { useToast } from "@/components/ToastProvider";
import ScrollReveal from "@/components/ScrollReveal";

function AnimatedCounter({ target, suffix = "", prefix = "", finalText }: { target: number; suffix?: string; prefix?: string; finalText?: string }) {
  const [display, setDisplay] = useState(prefix + "0" + suffix);
  const ref = useRef<HTMLSpanElement>(null);
  const animated = useRef(false);

  const animate = useCallback(() => {
    if (animated.current) return;
    animated.current = true;
    const duration = 1500;
    const start = performance.now();
    const step = (now: number) => {
      const progress = Math.min((now - start) / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3); // ease-out cubic
      const current = Math.round(eased * target);
      setDisplay(prefix + current + suffix);
      if (progress < 1) {
        requestAnimationFrame(step);
      } else if (finalText) {
        setDisplay(finalText);
      }
    };
    requestAnimationFrame(step);
  }, [target, prefix, suffix, finalText]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const obs = new IntersectionObserver(
      ([e]) => { if (e.isIntersecting) { animate(); obs.disconnect(); } },
      { threshold: 0.5 }
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [animate]);

  return <span ref={ref}>{display}</span>;
}

const PRIVACY_PROPERTIES = [
  {
    icon: "\u{1F6E1}",
    title: "Merchant Isolation",
    desc: "Merchants cannot compare records to identify the same beneficiary. Each connection is cryptographically unique.",
  },
  {
    icon: "\u{1F6AB}",
    title: "Fraud Prevention",
    desc: "The same person cannot register twice with any merchant. Duplicate attempts are automatically detected and rejected.",
  },
  {
    icon: "\u{1F50D}",
    title: "Privacy by Design",
    desc: "Beneficiaries prove their eligibility without revealing any personal information — not even which credential they hold.",
  },
  {
    icon: "\u26D3",
    title: "Bitcoin-Native Security",
    desc: "All privacy sets are recorded on Bitcoin using Taproot transactions, providing an immutable and transparent audit trail.",
  },
];

const STATS = [
  { value: 6, label: "Protocol Steps" },
  { value: 4, label: "Privacy Guarantees" },
  { value: 0, label: "Data Shared", finalText: "Zero" },
  { value: 33, label: "Byte Identity", prefix: "~", suffix: "B" },
];

const TECH_STACK = [
  { name: "Rust", desc: "Core cryptography, wallet binary, gRPC services" },
  { name: "BDK", desc: "bdk_wallet v2 for BIP86 P2TR descriptor wallets" },
  { name: "secp256k1", desc: "Elliptic curve operations, Pedersen commitments" },
  { name: "Bitcoin Core", desc: "Node for on-chain interaction via RPC" },
  { name: "tonic/prost", desc: "gRPC server and client implementation" },
  { name: "Next.js", desc: "React-based UI with server-side API routes" },
];

const PROTOCOL_STEPS = [
  {
    title: "Create Your Credential",
    desc: "Beneficiaries generate a private credential locally. Nothing leaves their device until they choose to register.",
  },
  {
    title: "Register Privately",
    desc: "The credential is added to a privacy set on Bitcoin. No personal information is stored or transmitted.",
  },
  {
    title: "Prove Without Revealing",
    desc: "When connecting to a merchant, the beneficiary proves they are registered without revealing which credential is theirs.",
  },
  {
    title: "Unique Identity Per Merchant",
    desc: "Each merchant sees a different pseudonym. Even if two merchants compare notes, they cannot identify the same beneficiary.",
  },
  {
    title: "One Identity, One Registration",
    desc: "Built-in safeguards ensure no one can register twice with the same merchant, preventing fraud without compromising privacy.",
  },
  {
    title: "Instant Authentication",
    desc: "After the first connection, beneficiaries authenticate with a single signature — fast, lightweight, and private.",
  },
];

export default function Home() {
  const router = useRouter();
  const { toast } = useToast();
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
      toast(
        "Popups blocked — click \"Allow\" in your browser's popup notification, then try again.",
        "error"
      );
    } else {
      toast(`Opened ${total} tabs`, "success");
    }
  }

  return (
    <>
      {/* Animated background */}
      <div className="landing-bg">
        <div className="veil-glow" />
        <div className="veil-ring veil-ring-1" />
        <div className="veil-ring veil-ring-2" />
        <div className="veil-threshold" />
      </div>

      <div className="landing-content" style={{ marginTop: "2rem", textAlign: "center" }}>
        {/* Shield hero */}
        <div className="shield-hero" style={{ margin: "0 auto 1.5rem", width: "200px", height: "230px" }}>
          <svg viewBox="0 0 120 140" fill="none" xmlns="http://www.w3.org/2000/svg" width="200" height="230">
            <defs>
              <linearGradient id="shield-grad" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stopColor="#F5A623" />
                <stop offset="100%" stopColor="#D4800A" />
              </linearGradient>
            </defs>
            {/* Shield outline */}
            <path
              d="M60 8 L108 30 L108 75 C108 105 85 128 60 136 C35 128 12 105 12 75 L12 30 Z"
              stroke="url(#shield-grad)" strokeWidth="2" fill="rgba(245,166,35,0.05)"
            />
            {/* Partial veil ring */}
            <circle cx="60" cy="68" r="36" stroke="#fff" strokeWidth="1.5" opacity="0.15" />
            <path d="M60 32 A36 36 0 0 1 96 68" stroke="#fff" strokeWidth="2.5" opacity="0.4" strokeLinecap="round" />
            <path d="M24 68 A36 36 0 0 1 60 32" stroke="#fff" strokeWidth="2.5" opacity="0.25" strokeLinecap="round" />
            {/* Bitcoin symbol */}
            <g transform="translate(60,68)">
              <rect x="-6" y="-18" width="2.5" height="36" rx="1.25" fill="url(#shield-grad)" />
              <rect x="3.5" y="-18" width="2.5" height="36" rx="1.25" fill="url(#shield-grad)" />
              <path d="M-12,-14 L4,-14 C12,-14 16,-10 16,-6 C16,-2 12,1 4,1 L-12,1 Z" fill="url(#shield-grad)" opacity="0.95" />
              <path d="M-12,-1 L6,-1 C15,-1 19,3 19,8 C19,13 15,17 6,17 L-12,17 Z" fill="url(#shield-grad)" opacity="0.95" />
              <path d="M-8,-10 L3,-10 C8,-10 10,-8 10,-6 C10,-4 8,-3 3,-3 L-8,-3 Z" fill="#0a0a0a" />
              <path d="M-8,3 L5,3 C10,3 13,5 13,8 C13,11 10,13 5,13 L-8,13 Z" fill="#0a0a0a" />
            </g>
          </svg>
        </div>
        <h2
          style={{
            fontSize: "clamp(1.1rem, 3vw, 1.4rem)",
            fontWeight: 600,
            marginBottom: "0.5rem",
            color: "#c0c0c0",
            letterSpacing: "0.03em",
          }}
        >
          Verified Payments. Veiled Identities.
        </h2>
        <p style={{ color: "#666", marginBottom: "2.5rem", fontSize: "1rem" }}>
          Secure, private payments between merchants and beneficiaries — powered by Bitcoin
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
              Register your business, verify beneficiaries securely, and send
              Bitcoin payments — no sensitive data exchanged.
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
            style={{ fontSize: "1rem", padding: "0.65rem 2.5rem" }}
          >
            Quick Preview
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

      {/* Why Veiled */}
      <ScrollReveal>
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
          <h2 style={{ textAlign: "center", fontSize: "clamp(1.2rem, 4vw, 1.6rem)", fontWeight: 700, marginBottom: "1.5rem" }}>
            Why Veiled
          </h2>
          <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem", marginBottom: "1rem" }}>
            Veiled enables merchants and beneficiaries to transact on Bitcoin without
            exposing personal information. Beneficiaries register once and connect to
            multiple merchants using unique, unlinkable pseudonyms — no sensitive data
            is ever shared between parties.
          </p>
          <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem", marginBottom: "1rem" }}>
            Each beneficiary creates a single credential and uses it to connect with
            any merchant. Every connection generates a unique identity that cannot be
            traced back to the beneficiary or linked across merchants. Built-in
            safeguards prevent anyone from registering twice with the same merchant.
          </p>
          <p style={{ color: "#999", lineHeight: 1.8, fontSize: "0.92rem" }}>
            Once connected, beneficiaries authenticate instantly and receive Bitcoin
            payments directly to their wallet — no further setup needed. The entire
            process is anchored on Bitcoin for transparency and tamper resistance.
          </p>
        </section>
      </ScrollReveal>

      {/* Stats */}
      <ScrollReveal>
        <div className="stats-row" style={{ justifyContent: "center", maxWidth: "700px", margin: "2rem auto" }}>
          {STATS.map((s, i) => (
            <div key={i} className="stat-card" style={{ textAlign: "center" }}>
              <div className="stat-value">
                <AnimatedCounter target={s.value} suffix={s.suffix} prefix={s.prefix} finalText={s.finalText} />
              </div>
              <div className="stat-label">{s.label}</div>
            </div>
          ))}
        </div>
      </ScrollReveal>

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
        <h2 style={{ textAlign: "center", fontSize: "clamp(1.2rem, 4vw, 1.6rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
          How It Works
        </h2>
        <p style={{ textAlign: "center", color: "#666", marginBottom: "2.5rem", fontSize: "0.9rem" }}>
          From registration to payment in six steps
        </p>
        <ScrollReveal>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))", gap: "1.25rem" }}>
            {PROTOCOL_STEPS.map((step, i) => (
              <div key={i} className="protocol-step">
                <div style={{ color: "#f5a623", fontSize: "0.75rem", fontWeight: 600, marginBottom: "0.5rem", letterSpacing: "0.05em", textTransform: "uppercase", display: "flex", alignItems: "center", gap: "0.4rem" }}>
                  <span style={{ width: "20px", height: "20px", borderRadius: "50%", border: "1.5px solid rgba(245, 166, 35, 0.4)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: "0.65rem" }}>
                    {i + 1}
                  </span>
                  Step {i + 1}
                </div>
                <h3 style={{ fontWeight: 600, marginBottom: "0.4rem", fontSize: "0.95rem" }}>{step.title}</h3>
                <p style={{ color: "#888", fontSize: "0.83rem", lineHeight: 1.6 }}>{step.desc}</p>
              </div>
            ))}
          </div>
        </ScrollReveal>
      </section>

      {/* Trust & Security */}
      <section
        id="privacy"
        style={{ padding: "3rem 1rem", borderTop: "1px solid #1a1a1a", scrollMarginTop: "60px" }}
      >
        <h2 style={{ textAlign: "center", fontSize: "clamp(1.2rem, 4vw, 1.6rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
          Trust &amp; Security
        </h2>
        <p style={{ textAlign: "center", color: "#666", marginBottom: "2.5rem", fontSize: "0.9rem" }}>
          Cryptographic guarantees that protect every transaction
        </p>
        <ScrollReveal>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))", gap: "1.25rem" }}>
            {PRIVACY_PROPERTIES.map((prop, i) => (
              <div key={i} className="privacy-card">
                <div style={{ fontSize: "1.6rem", marginBottom: "0.75rem" }}>{prop.icon}</div>
                <h3 style={{ fontWeight: 600, marginBottom: "0.4rem", fontSize: "0.95rem" }}>{prop.title}</h3>
                <p style={{ color: "#888", fontSize: "0.83rem", lineHeight: 1.6 }}>{prop.desc}</p>
              </div>
            ))}
          </div>
        </ScrollReveal>
      </section>

      {/* How It All Connects */}
      <section
        id="architecture"
        style={{
          padding: "3rem 1rem",
          borderTop: "1px solid #1a1a1a",
          background: "linear-gradient(180deg, transparent, #0d0d0d 40%)",
          scrollMarginTop: "60px",
        }}
      >
        <h2 style={{ textAlign: "center", fontSize: "clamp(1.2rem, 4vw, 1.6rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
          How It All Connects
        </h2>
        <p style={{ textAlign: "center", color: "#666", marginBottom: "2.5rem", fontSize: "0.9rem" }}>
          Beneficiaries, merchants, and the Bitcoin network working together
        </p>
        <ScrollReveal>
          <div style={{ maxWidth: "640px", margin: "0 auto" }}>
            <svg viewBox="0 0 640 280" fill="none" xmlns="http://www.w3.org/2000/svg" style={{ width: "100%", height: "auto" }}>
              {/* Nodes */}
              <rect x="20" y="40" width="150" height="60" rx="12" fill="#1a1a1a" stroke="#f5a623" strokeWidth="1.5" />
              <text x="95" y="75" textAnchor="middle" fill="#ededed" fontSize="14" fontWeight="600">Beneficiary</text>

              <rect x="245" y="40" width="150" height="60" rx="12" fill="#1a1a1a" stroke="#f5a623" strokeWidth="1.5" />
              <text x="320" y="75" textAnchor="middle" fill="#ededed" fontSize="14" fontWeight="600">Registry</text>

              <rect x="470" y="40" width="150" height="60" rx="12" fill="#1a1a1a" stroke="#f5a623" strokeWidth="1.5" />
              <text x="545" y="75" textAnchor="middle" fill="#ededed" fontSize="14" fontWeight="600">Merchant</text>

              {/* Connecting arrows */}
              <line x1="170" y1="70" x2="245" y2="70" className="flow-line" stroke="#f5a623" strokeWidth="1.5" />
              <polygon points="242,66 250,70 242,74" fill="#f5a623" />
              <text x="208" y="60" textAnchor="middle" fill="#888" fontSize="10">credential</text>

              <line x1="395" y1="70" x2="470" y2="70" className="flow-line" stroke="#f5a623" strokeWidth="1.5" />
              <polygon points="467,66 475,70 467,74" fill="#f5a623" />
              <text x="433" y="60" textAnchor="middle" fill="#888" fontSize="10">pseudonym</text>

              {/* Vertical lines down to Bitcoin */}
              <line x1="95" y1="100" x2="95" y2="190" stroke="#333" strokeWidth="1" strokeDasharray="4 3" />
              <line x1="320" y1="100" x2="320" y2="190" stroke="#333" strokeWidth="1" strokeDasharray="4 3" />
              <line x1="545" y1="100" x2="545" y2="190" stroke="#333" strokeWidth="1" strokeDasharray="4 3" />

              {/* Bitcoin anchor */}
              <rect x="170" y="190" width="300" height="60" rx="12" fill="#1a1a1a" stroke="#f87171" strokeWidth="1.5" />
              <text x="320" y="225" textAnchor="middle" fill="#f87171" fontSize="14" fontWeight="600">Bitcoin Network</text>

              {/* Side labels */}
              <text x="95" y="155" textAnchor="middle" fill="#666" fontSize="9">privacy set</text>
              <text x="320" y="155" textAnchor="middle" fill="#666" fontSize="9">Taproot anchor</text>
              <text x="545" y="155" textAnchor="middle" fill="#666" fontSize="9">verification</text>
            </svg>
          </div>
        </ScrollReveal>
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
        <h2 style={{ textAlign: "center", fontSize: "clamp(1.2rem, 4vw, 1.6rem)", fontWeight: 700, marginBottom: "0.5rem" }}>
          Built With
        </h2>
        <p style={{ textAlign: "center", color: "#666", marginBottom: "2.5rem", fontSize: "0.9rem" }}>
          Production-grade open-source tooling
        </p>
        <ScrollReveal>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: "1rem" }}>
            {TECH_STACK.map((tech, i) => (
              <div key={i} className="tech-card">
                <span className="tech-name">{tech.name}</span>
                <span className="tech-desc">{tech.desc}</span>
              </div>
            ))}
          </div>
        </ScrollReveal>
      </section>
    </>
  );
}
