import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import ToastProvider from "@/components/ToastProvider";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Veiled — Verified Payments, Veiled Identities",
  description:
    "Interactive demo of the Veiled pseudonymous payment protocol on Bitcoin",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased min-h-screen`}
        style={{ background: "#0a0a0a", color: "#ededed", display: "flex", flexDirection: "column" as const }}
      >
        <nav
          style={{
            borderBottom: "1px solid #222",
            padding: "0.75rem 1.5rem",
            display: "flex",
            alignItems: "center",
            gap: "0.75rem",
            position: "sticky",
            top: 0,
            zIndex: 50,
          }}
        >
          <a
            href="/"
            style={{
              display: "flex",
              alignItems: "center",
              textDecoration: "none",
              flexShrink: 0,
            }}
          >
            <img
              src="/logo-nav.svg"
              alt="Veiled"
              style={{ height: "32px", width: "auto" }}
            />
          </a>
          <span className="nav-tagline" style={{ color: "#666", fontSize: "0.85rem" }}>
            Verified Payments, Veiled Identities
          </span>
        </nav>
        {/* Subtle global background animation */}
        <div className="app-bg">
          <div className="grid-overlay" />
          <div
            className="glow-orb"
            style={{ opacity: 0.3 }}
          />
        </div>
        <main style={{ padding: "1rem 1.5rem", width: "100%", flex: 1, position: "relative", zIndex: 1 }}>
          <ToastProvider>{children}</ToastProvider>
        </main>
        <footer
          style={{
            borderTop: "1px solid #1a1a1a",
            padding: "2rem 1rem",
            textAlign: "center",
            color: "#444",
            fontSize: "0.8rem",
            lineHeight: 1.8,
            position: "relative",
            zIndex: 1,
          }}
        >
          <div style={{ marginBottom: "0.5rem" }}>
            <a href="/" style={{ textDecoration: "none" }}>
              <img
                src="/logo-nav.svg"
                alt="Veiled"
                style={{ height: "24px", width: "auto", opacity: 0.4, display: "inline-block" }}
              />
            </a>
          </div>
          <div>
            Anonymous Self-Credentials on Bitcoin &middot; ZK Proofs &middot; P2TR Payments
          </div>
          <div style={{ marginTop: "0.25rem", color: "#333" }}>
            secp256k1 &middot; Bootle/Groth one-out-of-many proofs &middot; Schnorr authentication
          </div>
        </footer>
      </body>
    </html>
  );
}
