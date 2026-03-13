"use client";

import { useState } from "react";

interface PhaseCardProps {
  title: string;
  badge?: string;
  active?: boolean;
  locked?: boolean;
  completed?: boolean;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

export default function PhaseCard({
  title,
  badge,
  active = false,
  locked = false,
  completed = false,
  defaultOpen = false,
  children,
}: PhaseCardProps) {
  const [open, setOpen] = useState(defaultOpen || active);

  const stateClass = completed
    ? "phase-card--completed"
    : active
    ? "phase-card--active"
    : locked
    ? "phase-card--locked"
    : "";

  return (
    <div className={`phase-card ${stateClass}`}>
      <button
        className="phase-card-header"
        onClick={() => !locked && setOpen(!open)}
        disabled={locked}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
          {badge && <span className="phase-badge">{badge}</span>}
          <h3 className="phase-card-title">{title}</h3>
          {completed && (
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" style={{ color: "#4ade80" }}>
              <path d="M3 8L6.5 11.5L13 5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
            </svg>
          )}
        </div>
        <span
          style={{
            transform: open ? "rotate(180deg)" : "rotate(0deg)",
            transition: "transform 0.2s",
            opacity: locked ? 0.3 : 0.6,
          }}
        >
          ▾
        </span>
      </button>
      {open && !locked && <div className="phase-card-body">{children}</div>}
    </div>
  );
}
