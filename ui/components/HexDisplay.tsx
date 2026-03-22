"use client";

import { useState, memo } from "react";

interface HexDisplayProps {
  value: string;
  label?: string;
  full?: boolean;
  truncate?: number;
}

export default memo(function HexDisplay({
  value,
  label,
  full = false,
  truncate = 8,
}: HexDisplayProps) {
  const [copied, setCopied] = useState(false);

  const displayValue = full
    ? value
    : value.length > truncate * 2
    ? `${value.slice(0, truncate)}...${value.slice(-truncate)}`
    : value;

  const handleCopy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <span className="hex-display" onClick={handleCopy} title={value}>
      {label && <span className="hex-label">{label}: </span>}
      <code className="hex-value">{displayValue}</code>
      <span className={`copy-btn ${copied ? "copy-btn--copied" : ""}`}>
        {copied ? "✓" : "⧉"}
      </span>
    </span>
  );
});
