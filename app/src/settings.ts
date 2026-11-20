// App preferences, kept in this machine's browser storage (they only matter to this install).
import { useSyncExternalStore } from "react";

export type ThemeChoice = "system" | "light" | "dark";

export interface Settings {
  theme: ThemeChoice;
  reduceMotion: boolean;
  /** Check GitHub for a new app version on launch. */
  autoUpdate: boolean;
}

const DEFAULTS: Settings = {
  theme: "system",
  reduceMotion: false,
  autoUpdate: true,
};
const KEY = "moddedmc.settings";

function load(): Settings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

let current = load();
const listeners = new Set<() => void>();

export function getSettings(): Settings {
  return current;
}

export function setSetting<K extends keyof Settings>(key: K, value: Settings[K]) {
  current = { ...current, [key]: value };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Storage can be unavailable; the choice still holds for this session.
  }
  applyAppearance();
  for (const listener of listeners) listener();
}

export function useSettings(): Settings {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

/** Put the theme and motion choices on <html>. */
export function applyAppearance() {
  const dark = current.theme === "dark" || (current.theme === "system" && darkQuery.matches);
  const root = document.documentElement;
  root.classList.toggle("dark", dark);
  root.classList.toggle("reduce-motion", current.reduceMotion);
}

darkQuery.addEventListener("change", applyAppearance);
