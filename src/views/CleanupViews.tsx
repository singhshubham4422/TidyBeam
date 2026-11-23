import { useEffect, useState } from "react";
import { Check, Copy, LoaderCircle, RefreshCw, ShieldCheck, Sparkles } from "lucide-react";
import { api, errorText, fmtBytes, fmtDate, type CleanupCandidate, type DuplicateGroup, type FileEntry } from "../api";
import { FileIcon } from "../components/FileIcon";

type TrashFn = (paths: string[], what: string) => Promise<boolean>;

export function DuplicatesView({ path, trashName, onTrash, onOpen }: { path: string; trashName: string; onTrash: TrashFn; onOpen: (entry: FileEntry) => void }) {
  const [target, setTarget] = useState(path);
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const run = (folder: string) => {
    setTarget(folder); setBusy(true); setError(""); setGroups(null);
    api.findDuplicates(folder).then(setGroups).catch(e => setError(errorText(e))).finally(() => setBusy(false));
  };
  useEffect(() => { run(path); }, [path]);

  const keepNewest = async (group: DuplicateGroup) => {
    const extra = [...group.files].sort((a, b) => b.modified - a.modified).slice(1);
    const ok = await onTrash(extra.map(f => f.path), `${extra.length} duplicate${extra.length === 1 ? "" : "s"} of “${group.files[0].name}”`);
    if (ok) setGroups(old => old?.filter(g => g.hash !== group.hash) ?? null);
  };
  const recoverable = groups?.reduce((sum, g) => sum + g.size * (g.files.length - 1), 0) ?? 0;

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow truncate">Duplicates · {target}</div>
          <h1 className="big">{groups ? fmtBytes(recoverable) : busy ? "Scanning…" : "—"}</h1>
          <p>{groups ? `${groups.length} group${groups.length === 1 ? "" : "s"} of identical files could be cleaned up` : "Comparing size, samples, then full contents"}</p>
        </div>
        <div className="actions">
          <button type="button" className="btn" onClick={() => run(path)} disabled={busy}>Use current folder</button>
          <button type="button" className="btn btn-dark" onClick={() => run(target)} disabled={busy}>{busy ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Scan again</button>
        </div>
      </header>
      <div className="notice"><ShieldCheck size={16} /><span>Files only count as duplicates when their full contents match. “Keep newest” asks before moving the other copies to the {trashName}.</span></div>
      {error && <div className="state error"><h3>Couldn't scan this folder</h3><p>{error}</p></div>}
      {busy && <div className="state"><LoaderCircle size={22} className="spin" /><span>Finding exact duplicates…</span></div>}
      {groups && groups.length === 0 && <div className="state"><span className="badge"><Check size={20} /></span><h3>No duplicates</h3><p>Every file here is unique.</p></div>}
      {groups?.map(group => {
        const newest = [...group.files].sort((a, b) => b.modified - a.modified)[0];
        return (
          <section key={group.hash} className="panel">
            <h2><span><Copy size={15} style={{ verticalAlign: "-2px", marginRight: 8 }} aria-hidden="true" />{group.files.length} copies · {fmtBytes(group.size)} each</span><button type="button" className="btn btn-sm" onClick={() => void keepNewest(group)}>Keep newest</button></h2>
            {group.files.map(file => (
              <button key={file.path} type="button" className="panel-row clickable" onDoubleClick={() => onOpen(file)} title={file.path}>
                <FileIcon entry={file} />
                <span className="grow"><span className="truncate" style={{ display: "block" }}>{file.name}</span><span className="sub truncate">{file.path}</span></span>
                <span className="muted small">{fmtDate(file.modified)}</span>
                {file.path === newest.path && <span className="pill live">Newest</span>}
              </button>
            ))}
          </section>
        );
      })}
    </div>
  );
}

export function CleanupView({ trashName, onTrash }: { trashName: string; onTrash: TrashFn }) {
  const [items, setItems] = useState<CleanupCandidate[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const scan = () => {
    setBusy(true); setError(""); setSelected(new Set());
    api.scanCleanup().then(setItems).catch(e => setError(errorText(e))).finally(() => setBusy(false));
  };
  useEffect(scan, []);

  const total = items?.reduce((sum, item) => sum + item.bytes, 0) ?? 0;
  const chosen = items?.filter(item => selected.has(item.path)) ?? [];
  const clean = async () => {
    const ok = await onTrash(chosen.map(i => i.path), `${chosen.length} temporary item${chosen.length === 1 ? "" : "s"} (${fmtBytes(chosen.reduce((s, i) => s + i.bytes, 0))})`);
    if (ok) scan();
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Cleanup · temporary files older than 7 days</div>
          <h1 className="big">{items ? fmtBytes(total) : busy ? "Scanning…" : "—"}</h1>
          <p>{items ? `${items.length} item${items.length === 1 ? "" : "s"} you can review` : "Only your own temporary files are listed"}</p>
        </div>
        <div className="actions">
          <button type="button" className="btn" onClick={scan} disabled={busy}>{busy ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Scan again</button>
          <button type="button" className="btn btn-dark" disabled={!chosen.length} onClick={() => void clean()}>Move {chosen.length || ""} to {trashName}</button>
        </div>
      </header>
      <div className="notice"><ShieldCheck size={16} /><span>Nothing is removed until you select it and confirm. Items you clean go to the {trashName}, so you can restore them.</span></div>
      {error && <div className="state error"><h3>Couldn't scan</h3><p>{error}</p></div>}
      {items && items.length === 0 && <div className="state"><span className="badge"><Sparkles size={20} /></span><h3>Nothing to clean</h3><p>No temporary items older than 7 days.</p></div>}
      {items && items.length > 0 && (
        <section className="panel">
          <h2>
            <span>Temporary items</span>
            <button type="button" className="btn btn-sm" onClick={() => setSelected(selected.size === items.length ? new Set() : new Set(items.map(i => i.path)))}>{selected.size === items.length ? "Select none" : "Select all"}</button>
          </h2>
          {items.map(item => (
            <label key={item.path} className="check-row" title={item.path}>
              <input type="checkbox" checked={selected.has(item.path)} onChange={event => setSelected(old => { const next = new Set(old); if (event.target.checked) next.add(item.path); else next.delete(item.path); return next; })} />
              <span className="grow" style={{ flex: 1, minWidth: 0 }}><span className="truncate" style={{ display: "block" }}>{item.name}</span><span className="sub muted small">{item.category} · {item.item_count.toLocaleString()} file{item.item_count === 1 ? "" : "s"} · last changed {fmtDate(item.modified)}</span></span>
              <span className="num">{fmtBytes(item.bytes)}</span>
            </label>
          ))}
        </section>
      )}
    </div>
  );
}
