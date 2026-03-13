// Hook for persisting state to localStorage.
// On mount, restores saved values. On state change, saves to localStorage.

import { useState, useEffect, useCallback } from "react";

const PREFIX = "veiled:";

export function useLocalState<T>(key: string, initial: T): [T, (val: T | ((prev: T) => T)) => void, () => void] {
  const storageKey = PREFIX + key;

  const [value, setValue] = useState<T>(() => {
    if (typeof window === "undefined") return initial;
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored) return JSON.parse(stored);
    } catch {
      // ignore parse errors
    }
    return initial;
  });

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(value));
    } catch {
      // ignore quota errors
    }
  }, [storageKey, value]);

  const clear = useCallback(() => {
    localStorage.removeItem(storageKey);
    setValue(initial);
  }, [storageKey, initial]);

  return [value, setValue, clear];
}

export function clearAllLocalState() {
  if (typeof window === "undefined") return;
  const keys = Object.keys(localStorage).filter((k) => k.startsWith(PREFIX));
  keys.forEach((k) => localStorage.removeItem(k));
}
