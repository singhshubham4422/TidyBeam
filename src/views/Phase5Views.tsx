import { useEffect, useMemo, useState } from "react";
import { Archive, ExternalLink, HardDrive, LoaderCircle, Package, Search, ShieldCheck, Trash2 } from "lucide-react";
import { api, errorText, fmtBytes, type DiskHealthInfo, type InstalledApp, type LeftoverFolder, type PlatformInfo } from "../api";

type TrashFn = (paths: string[], what: string) => Promise<string[]>;

export function AppsView({ trashName, onTrash }: { trashName: string; onTrash: TrashFn }) {
  const [apps, setApps] = useState<InstalledApp[] | null>(null);
  const [query, setQuery] = useState("");
  const [leftovers, setLeftovers] = useState<{ app: string; folders: LeftoverFolder[] } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setBusy(true);
    api.listInstalledApps()
      .then(setApps)
      .catch(() => setApps([]))
      .finally(() => setBusy(false));
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (apps ?? []).filter(a => !q || a.name.toLowerCase().includes(q) || a.publisher.toLowerCase().includes(q)).slice(0, 150);
  }, [apps, query]);

  const scanLeftovers = async (appName: string) => {
    const folders = await api.findAppLeftovers(appName);
    setLeftovers({ app: appName, folders });
  };

  const trashLeftover = async (folder: LeftoverFolder) => {
    const removed = await onTrash([folder.path], `“${folder.name}”`);
    if (removed.length > 0) {
      setLeftovers(old => old ? { ...old, folders: old.folders.filter(f => f.path !== folder.path) } : null);
    }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Installed Apps · Local Inventory & Leftover Scanner</div>
          <h1 className="big">{apps ? `${apps.length} apps` : busy ? "Loading…" : "—"}</h1>
          <p>Inspect installed software and scan for orphaned app-data folders.</p>
        </div>
        <label className="filter-box">
          <Search size={14} aria-hidden="true" />
          <input value={query} onChange={e => setQuery(e.target.value)} placeholder="Filter apps…" />
        </label>
      </header>

      {leftovers && (
        <section className="panel" style={{ marginBottom: 12 }}>
          <h2>
            <span>App Data Matches for “{leftovers.app}” ({leftovers.folders.length})</span>
            <button type="button" className="btn btn-sm" onClick={() => setLeftovers(null)}>Close</button>
          </h2>
          {leftovers.folders.length === 0 ? (
            <div className="state"><p>No matching app-data folders found.</p></div>
          ) : leftovers.folders.map(f => (
            <div key={f.path} className="panel-row" title={f.path}>
              <span className="grow"><strong className="truncate" style={{ display: "block" }}>{f.name}</strong><span className="sub truncate">{f.path}</span></span>
              <span className="num">{fmtBytes(f.bytes)}</span>
              <button type="button" className="btn btn-sm" onClick={() => void trashLeftover(f)}><Trash2 size={13} /> Move to {trashName}</button>
            </div>
          ))}
        </section>
      )}

      <section className="panel">
        {busy && !apps && <div className="state"><LoaderCircle size={20} className="spin" /><span>Listing applications…</span></div>}
        {filtered.map((app, i) => (
          <div key={`${app.name}-${i}`} className="panel-row" title={app.location}>
            <Package size={16} aria-hidden="true" />
            <span className="grow" style={{ minWidth: 0 }}>
              <strong className="truncate" style={{ display: "block" }}>{app.name} {app.version && <span className="muted small">v{app.version}</span>}</strong>
              <span className="sub truncate">{app.publisher || app.source}</span>
            </span>
            <button type="button" className="btn btn-sm" onClick={() => void scanLeftovers(app.name)}>Scan App Data</button>
          </div>
        ))}
      </section>
    </div>
  );
}

export function OsToolsBackupView({ currentPath, platform }: { currentPath: string; platform: PlatformInfo }) {
  const [disks, setDisks] = useState<DiskHealthInfo[]>([]);
  const [backupDest, setBackupDest] = useState(currentPath);
  const [backupName, setBackupName] = useState("TidyBeam-Backup.zip");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    void api.listDiskHealth().then(setDisks).catch(() => undefined);
  }, []);

  const runBackup = async () => {
    setBusy(true); setMsg(""); setError("");
    try {
      const out = await api.backupFolder(currentPath, backupDest, backupName);
      setMsg(`Backup saved to ${out}`);
    } catch (e) { setError(errorText(e)); }
    finally { setBusy(false); }
  };

  const openOs = (target: string) => {
    void api.openOsSettings(target).catch(e => setError(errorText(e)));
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Disk Health, Folder Backup & OS Maintenance</div>
          <h1 className="big">{disks.length} drive{disks.length === 1 ? "" : "s"}</h1>
          <p>Offline folder backups and standard-user deep-links to native OS security & update settings.</p>
        </div>
      </header>
      {msg && <div className="notice"><ShieldCheck size={16} /><span>{msg}</span></div>}
      {error && <div className="state error"><h3>Action failed</h3><p>{error}</p></div>}

      <div className="columns">
        <section className="panel" style={{ flex: 1 }}>
          <h2><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><HardDrive size={15} /> Storage Drives</span></h2>
          {disks.map(d => (
            <div key={d.mountPoint} className="panel-row">
              <span className="grow">
                <strong>{d.name || d.mountPoint} <span className="muted small">({d.kind} · {d.fileSystem})</span></strong>
                <span className="sub">{fmtBytes(d.availableBytes)} free of {fmtBytes(d.totalBytes)} ({d.usedPct}% used)</span>
              </span>
              <span className={`pill ${d.status === "Healthy" ? "live" : ""}`}>{d.status}</span>
            </div>
          ))}
        </section>

        <section className="panel" style={{ flex: 1, padding: 14 }}>
          <h2 style={{ padding: 0, marginBottom: 10 }}><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Archive size={15} /> Quick Folder ZIP Backup</span></h2>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div className="muted small">Source: {currentPath}</div>
            <input className="path-input" value={backupDest} onChange={e => setBackupDest(e.target.value)} placeholder="Destination folder…" />
            <input className="path-input" value={backupName} onChange={e => setBackupName(e.target.value)} placeholder="Backup filename (.zip)" />
            <button type="button" className="btn btn-dark" disabled={busy} onClick={() => void runBackup()}>
              {busy ? <LoaderCircle size={15} className="spin" /> : <Archive size={15} />} Create Backup ZIP
            </button>
          </div>

          <h2 style={{ padding: 0, margin: "16px 0 8px" }}>OS Security & Update Shortcuts</h2>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
            {platform.os === "windows" && (
              <>
                <button type="button" className="btn btn-sm" onClick={() => openOs("windows-security")}><ExternalLink size={13} /> Windows Security</button>
                <button type="button" className="btn btn-sm" onClick={() => openOs("windows-update")}><ExternalLink size={13} /> Windows Update</button>
                <button type="button" className="btn btn-sm" onClick={() => openOs("windows-apps")}><ExternalLink size={13} /> Apps & Features</button>
              </>
            )}
            {platform.os === "macos" && (
              <>
                <button type="button" className="btn btn-sm" onClick={() => openOs("macos-update")}><ExternalLink size={13} /> Software Update</button>
                <button type="button" className="btn btn-sm" onClick={() => openOs("macos-privacy")}><ExternalLink size={13} /> Full Disk Access</button>
              </>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
