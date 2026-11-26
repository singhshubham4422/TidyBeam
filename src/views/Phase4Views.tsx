import { useEffect, useState } from "react";
import { AlertTriangle, FileCode, History, LoaderCircle, RefreshCw, ShieldAlert, Trash2 } from "lucide-react";
import { api, errorText, fmtBytes, fmtDate, type AuditEntry, type DevBloatItem, type ImportedCleanerRule } from "../api";
import type { ConfirmOptions } from "../components/Dialogs";

type TrashFn = (paths: string[], what: string) => Promise<string[]>;

export function DevBloatView({ path, trashName, onTrash }: { path: string; trashName: string; onTrash: TrashFn }) {
  const [days, setDays] = useState(30);
  const [items, setItems] = useState<DevBloatItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const scan = () => {
    setBusy(true);
    setError("");
    api.scanDevBloat(path, days)
      .then(setItems)
      .catch(e => setError(errorText(e)))
      .finally(() => setBusy(false));
  };

  useEffect(() => { scan(); }, [path, days]);

  const total = items?.reduce((s, i) => s + i.bytes, 0) ?? 0;
  const trashOne = async (item: DevBloatItem) => {
    const removed = await onTrash([item.path], `“${item.name}”`);
    if (removed.length > 0) setItems(old => old?.filter(i => i.path !== item.path) ?? null);
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow truncate">Dev & Large Files · {path}</div>
          <h1 className="big">{items ? fmtBytes(total) : busy ? "Scanning…" : "—"}</h1>
          <p>Inactive <code>node_modules</code>, Rust <code>target/</code>, Python <code>.venv</code>, and old installers untouched for {days}+ days.</p>
        </div>
        <div className="actions">
          <select className="select" aria-label="Inactive days" value={days} onChange={e => setDays(Number(e.target.value))}>
            <option value={7}>7+ days inactive</option>
            <option value={30}>30+ days inactive</option>
            <option value={90}>90+ days inactive</option>
          </select>
          <button type="button" className="btn btn-dark" onClick={scan} disabled={busy}>{busy ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Rescan</button>
        </div>
      </header>
      {error && <div className="state error"><h3>Scan failed</h3><p>{error}</p></div>}
      {items && items.length === 0 && <div className="state"><FileCode size={22} /><h3>No inactive dev bloat found</h3><p>Active projects are preserved.</p></div>}
      {items && items.length > 0 && (
        <section className="panel">
          {items.map(item => (
            <div key={item.path} className="panel-row" title={item.path}>
              <FileCode size={17} aria-hidden="true" />
              <span className="grow" style={{ minWidth: 0 }}>
                <strong className="truncate" style={{ display: "block" }}>{item.name}</strong>
                <span className="sub truncate">{item.kind} · last modified {fmtDate(item.modified)}</span>
              </span>
              <span className="num">{fmtBytes(item.bytes)}</span>
              <button type="button" className="btn btn-sm" onClick={() => void trashOne(item)}><Trash2 size={13} /> Move to {trashName}</button>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}

export function PrivacyShredView({ currentPath, confirm }: { currentPath: string; confirm: (o: ConfirmOptions) => Promise<boolean> }) {
  const [targetPath, setTargetPath] = useState("");
  const [passes, setPasses] = useState(1);
  const [wipeMb, setWipeMb] = useState(256);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [error, setError] = useState("");

  const shred = async () => {
    if (!targetPath.trim()) return;
    const ok = await confirm({
      title: "Permanently shred this item?",
      body: "Shredding overwrites and deletes the file or folder without using Trash. Note: SSD wear-leveling and copy-on-write filesystems (APFS/Btrfs/ZFS) may retain prior blocks; use full-disk encryption for maximum security.",
      confirmLabel: "Shred Permanently",
      danger: true,
    });
    if (!ok) return;
    setBusy(true); setError(""); setMsg("");
    try {
      const count = await api.shredPaths([targetPath.trim()], passes);
      setMsg(`Shredded ${count} file(s) with ${passes} overwrite pass(es).`);
      setTargetPath("");
    } catch (e) { setError(errorText(e)); }
    finally { setBusy(false); }
  };

  const wipeFree = async () => {
    const ok = await confirm({
      title: `Wipe up to ${wipeMb} MB of free space?`,
      body: "Writes temporary zero-filled files and removes them. On SSDs or CoW filesystems (APFS, Btrfs, ZFS), physical block erasure is not guaranteed.",
      confirmLabel: "Wipe Free Space",
      danger: true,
    });
    if (!ok) return;
    setBusy(true); setError(""); setMsg("");
    try {
      const bytes = await api.wipeFreeSpace(currentPath, wipeMb);
      setMsg(`Wiped ${fmtBytes(bytes)} of free space in ${currentPath}.`);
    } catch (e) { setError(errorText(e)); }
    finally { setBusy(false); }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Privacy & Secure Sanitization</div>
          <h1 className="big">File Shredder & Free Space</h1>
          <p>Explicit opt-in overwrite-before-unlink without following symbolic links.</p>
        </div>
      </header>
      <div className="notice" style={{ borderColor: "var(--warning, #d97706)" }}>
        <AlertTriangle size={16} />
        <span><strong>SSD & CoW Notice:</strong> Solid-state drives (wear-leveling), copy-on-write filesystems (APFS, Btrfs, ZFS), and Volume Shadow Copies prevent guaranteed physical erasure. Enable BitLocker, FileVault, or LUKS full-disk encryption for sensitive data.</span>
      </div>
      {msg && <div className="notice"><ShieldAlert size={16} /><span>{msg}</span></div>}
      {error && <div className="state error"><h3>Operation failed</h3><p>{error}</p></div>}

      <div className="columns">
        <section className="panel" style={{ flex: 1, padding: 14 }}>
          <h2 style={{ padding: 0, marginBottom: 10 }}>Shred File or Folder</h2>
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <input className="path-input" value={targetPath} onChange={e => setTargetPath(e.target.value)} placeholder="Absolute path to file or folder…" />
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <select className="select" value={passes} onChange={e => setPasses(Number(e.target.value))}>
                <option value={1}>1 pass (0xAA)</option>
                <option value={2}>2 passes (0xAA, 0x55)</option>
                <option value={3}>3 passes (0xAA, 0x55, 0x00)</option>
              </select>
              <button type="button" className="btn danger" disabled={!targetPath.trim() || busy} onClick={() => void shred()}>Shred Permanently</button>
            </div>
          </div>
        </section>
        <section className="panel" style={{ flex: 1, padding: 14 }}>
          <h2 style={{ padding: 0, marginBottom: 10 }}>Free-Space Wipe ({currentPath})</h2>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <select className="select" value={wipeMb} onChange={e => setWipeMb(Number(e.target.value))}>
              <option value={128}>128 MB sample</option>
              <option value={256}>256 MB sample</option>
              <option value={512}>512 MB sample</option>
              <option value={1024}>1024 MB sample</option>
            </select>
            <button type="button" className="btn danger" disabled={busy} onClick={() => void wipeFree()}>Wipe Free Space</button>
          </div>
        </section>
      </div>
    </div>
  );
}

export function CustomCleanersView() {
  const [filePath, setFilePath] = useState("");
  const [rules, setRules] = useState<ImportedCleanerRule[]>([]);
  const [error, setError] = useState("");

  const loadFile = async () => {
    if (!filePath.trim()) return;
    setError("");
    try {
      const parsed = await api.importCustomCleaners(filePath.trim());
      setRules(parsed);
    } catch (e) { setError(errorText(e)); }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Custom Cleaners · CleanerML & winapp2.ini</div>
          <h1 className="big">{rules.length} imported rules</h1>
          <p>Import local <code>winapp2.ini</code> or CleanerML XML files completely offline.</p>
        </div>
      </header>
      <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
        <input className="path-input" style={{ flex: 1 }} value={filePath} onChange={e => setFilePath(e.target.value)} placeholder="Path to local winapp2.ini or cleaner.xml…" />
        <button type="button" className="btn btn-dark" onClick={() => void loadFile()}>Import Local File</button>
      </div>
      {error && <div className="state error"><h3>Import failed</h3><p>{error}</p></div>}
      <section className="panel">
        {rules.length === 0 ? (
          <div className="state"><p>No custom rules imported yet.</p></div>
        ) : rules.slice(0, 100).map((r, i) => (
          <div key={`${r.name}-${i}`} className="panel-row">
            <span className="grow">
              <strong>{r.name}</strong>
              <span className="sub">{r.section} · {r.targets.join(", ")}</span>
            </span>
            <span className="pill">{r.sourceFormat}</span>
          </div>
        ))}
      </section>
    </div>
  );
}

export function AuditLogView() {
  const [entries, setEntries] = useState<AuditEntry[]>([]);

  const load = () => { void api.listAuditLog(100).then(setEntries).catch(() => undefined); };
  useEffect(() => { load(); }, []);

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">History Log · Local SQLite Audit</div>
          <h1 className="big">{entries.length} events</h1>
          <p>Local record of cleanups, shreds, and free-space wipes.</p>
        </div>
        <button type="button" className="btn" onClick={load}><RefreshCw size={15} /> Refresh</button>
      </header>
      <section className="panel">
        {entries.length === 0 ? (
          <div className="state"><History size={22} /><h3>No history yet</h3><p>Cleanup and shred actions will be recorded locally here.</p></div>
        ) : entries.map(e => (
          <div key={e.id} className="panel-row">
            <History size={16} aria-hidden="true" />
            <span className="grow">
              <strong>{e.summary}</strong>
              <span className="sub">{e.action} · {fmtDate(e.timestamp)}</span>
            </span>
            <span className="num">{e.bytesReclaimed > 0 ? fmtBytes(e.bytesReclaimed) : `${e.itemsCount} items`}</span>
          </div>
        ))}
      </section>
    </div>
  );
}
