import { useCallback, useEffect, useState } from "react";
import type { SortKey } from "./api";

export type Theme = "system" | "light" | "dark";
export type Accent = "blue" | "teal" | "violet" | "graphite";

export type Settings = {
  theme: Theme;
  accent: Accent;
  compact: boolean;
  showHidden: boolean;
  confirmTrash: boolean;
  showDetails: boolean;
  view: "list" | "grid";
  sort: SortKey;
  descending: boolean;
  favorites: string[];
  globalShortcutEnabled: boolean;
  globalShortcut: string;
};

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  accent: "blue",
  compact: false,
  showHidden: false,
  confirmTrash: true,
  showDetails: true,
  view: "list",
  sort: "name",
  descending: false,
  favorites: [],
  globalShortcutEnabled: false,
  globalShortcut: "CommandOrControl+Shift+Space",
};

const KEY = "tidybeam-settings-v1";

function load(): Settings {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) || "{}");
    if (!raw || typeof raw !== "object") return DEFAULT_SETTINGS;
    const value = raw as Record<string, unknown>;
    const pick = <K extends keyof Settings>(key: K, ok: (v: unknown) => boolean): Settings[K] =>
      ok(value[key]) ? (value[key] as Settings[K]) : DEFAULT_SETTINGS[key];
    const isBool = (v: unknown) => typeof v === "boolean";
    return {
      theme: pick("theme", v => v === "system" || v === "light" || v === "dark"),
      accent: pick("accent", v => v === "blue" || v === "teal" || v === "violet" || v === "graphite"),
      compact: pick("compact", isBool),
      showHidden: pick("showHidden", isBool),
      confirmTrash: pick("confirmTrash", isBool),
      showDetails: pick("showDetails", isBool),
      view: pick("view", v => v === "list" || v === "grid"),
      sort: pick("sort", v => v === "name" || v === "size" || v === "modified" || v === "kind"),
      descending: pick("descending", isBool),
      favorites: pick("favorites", v => Array.isArray(v) && v.every(item => typeof item === "string")),
      globalShortcutEnabled: pick("globalShortcutEnabled", isBool),
      globalShortcut: pick("globalShortcut", v => typeof v === "string" && v.length > 0 && v.length < 64),
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function useSettings() {
  const [settings, setSettings] = useState<Settings>(load);
  useEffect(() => {
    try { localStorage.setItem(KEY, JSON.stringify(settings)); } catch { /* storage may be unavailable */ }
  }, [settings]);
  const update = useCallback((patch: Partial<Settings>) => setSettings(old => ({ ...old, ...patch })), []);
  return [settings, update] as const;
}

/** Applies theme, accent and density to the document root. */
export function useAppearance(settings: Settings) {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = settings.theme === "dark" || (settings.theme === "system" && media.matches);
      const root = document.documentElement;
      root.dataset.theme = dark ? "dark" : "light";
      root.dataset.accent = settings.accent;
      root.dataset.density = settings.compact ? "compact" : "comfortable";
      root.style.colorScheme = dark ? "dark" : "light";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings.theme, settings.accent, settings.compact]);
}
