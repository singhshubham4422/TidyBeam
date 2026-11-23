import { useState } from "react";
import { Laptop, Moon, Sun } from "lucide-react";
import type { PlatformInfo } from "../api";
import type { Accent, Settings } from "../settings";

type Props = { settings: Settings; update: (patch: Partial<Settings>) => void; platform: PlatformInfo; shortcutError: string };

const ACCENTS: { id: Accent; label: string; color: string }[] = [
  { id: "blue", label: "Blue", color: "#2563eb" },
  { id: "teal", label: "Teal", color: "#0f766e" },
  { id: "violet", label: "Violet", color: "#6d28d9" },
  { id: "graphite", label: "Graphite", color: "#3f3f46" },
];

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (value: boolean) => void; label: string }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} className="switch" onClick={() => onChange(!checked)} />;
}

export function SettingsView({ settings, update, platform, shortcutError }: Props) {
  const [shortcut, setShortcut] = useState(settings.globalShortcut);
  const row = (title: string, description: string, control: React.ReactNode) => (
    <div className="setting"><div className="label"><strong>{title}</strong><small>{description}</small></div>{control}</div>
  );
  return (
    <div className="tool">
      <header className="tool-head"><div><h1>Settings</h1><p>Preferences are saved on this device.</p></div></header>
      <div className="settings" style={{ display: "grid", gap: 20 }}>
        <section className="settings-section" aria-labelledby="appearance">
          <h2 id="appearance">Appearance</h2>
          {row("Theme", "Follow your system, or pick light or dark.", (
            <div className="seg" role="group" aria-label="Theme">
              <button type="button" aria-pressed={settings.theme === "system"} onClick={() => update({ theme: "system" })}><Laptop size={14} /> System</button>
              <button type="button" aria-pressed={settings.theme === "light"} onClick={() => update({ theme: "light" })}><Sun size={14} /> Light</button>
              <button type="button" aria-pressed={settings.theme === "dark"} onClick={() => update({ theme: "dark" })}><Moon size={14} /> Dark</button>
            </div>
          ))}
          {row("Accent color", "Used for selection, buttons and highlights.", (
            <div className="swatches" role="group" aria-label="Accent color">
              {ACCENTS.map(accent => <button key={accent.id} type="button" className="swatch" style={{ background: accent.color }} aria-label={accent.label} aria-pressed={settings.accent === accent.id} onClick={() => update({ accent: accent.id })} />)}
            </div>
          ))}
          {row("Compact rows", "Fit more items on screen in list view.", <Switch label="Compact rows" checked={settings.compact} onChange={compact => update({ compact })} />)}
        </section>

        <section className="settings-section" aria-labelledby="files">
          <h2 id="files">Files</h2>
          {row("Show hidden files", platform.os === "windows" ? "Includes items marked hidden or system." : "Includes items whose names start with a dot.", <Switch label="Show hidden files" checked={settings.showHidden} onChange={showHidden => update({ showHidden })} />)}
          {row(`Ask before moving to ${platform.trashName}`, "Recommended. Permanent deletes always ask.", <Switch label={`Ask before moving to ${platform.trashName}`} checked={settings.confirmTrash} onChange={confirmTrash => update({ confirmTrash })} />)}
          {row("Details panel", "Show a preview and details next to the file list.", <Switch label="Details panel" checked={settings.showDetails} onChange={showDetails => update({ showDetails })} />)}
        </section>

        <section className="settings-section" aria-labelledby="shortcuts">
          <h2 id="shortcuts">Shortcuts</h2>
          {row("Global search shortcut", platform.os === "linux" ? "Opens search from anywhere. Not available on some Wayland desktops." : "Opens search from anywhere, even when TidyBeam isn't focused.", <Switch label="Global search shortcut" checked={settings.globalShortcutEnabled} onChange={globalShortcutEnabled => update({ globalShortcutEnabled })} />)}
          {settings.globalShortcutEnabled && (
            <div className="setting">
              <label className="field" style={{ flex: 1 }}>
                <span>Key combination</span>
                <span className="input-row">
                  <input className="input" value={shortcut} onChange={e => setShortcut(e.target.value)} spellCheck={false} placeholder="CommandOrControl+Shift+Space" />
                  <button type="button" className="btn" disabled={!shortcut.trim() || shortcut === settings.globalShortcut} onClick={() => update({ globalShortcut: shortcut.trim() })}>Apply</button>
                </span>
                {shortcutError ? <span className="error-text">{shortcutError}</span> : <span className="hint">For example CommandOrControl+Shift+Space or Alt+Shift+F.</span>}
              </label>
            </div>
          )}
          {row("In-app shortcuts", "", (
            <span className="muted small" style={{ textAlign: "right" }}>{platform.os === "macos" ? "⌘K search · ⌘L location · ⌘[ ⌘] back/forward" : "Ctrl+K search · Ctrl+L location · Alt+← → back/forward"}</span>
          ))}
        </section>

        <section className="settings-section" aria-labelledby="about">
          <h2 id="about">About</h2>
          {row("TidyBeam", `Version ${platform.version} · open source under the MIT License`, <span />)}
        </section>
      </div>
    </div>
  );
}
