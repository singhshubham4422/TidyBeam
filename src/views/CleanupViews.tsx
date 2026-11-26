import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertTriangle, Check, ChevronDown, ChevronRight, Copy, Database, LoaderCircle, RefreshCw, Search, ShieldCheck, Sparkles, Trash2 } from "lucide-react";
import { api, errorText, fmtBytes, fmtDate, type CleanerFileItem, type CleanerRuleSummary, type DuplicateGroup, type FileEntry, type SafetyBadge } from "../api";
import { FileIcon } from "../components/FileIcon";

type TrashFn = (paths: string[], what: string) => Promise<string[]>;

let cachedDupeTarget = "";
let cachedDupeGroups: DuplicateGroup[] | null = null;
let cachedDeepRules: CleanerRuleSummary[] | null = null;

type FlatDupeRow =
  | { kind: "header"; group: DuplicateGroup }
  | { kind: "file"; group: DuplicateGroup; file: FileEntry; isNewest: boolean };

export function DuplicatesView({ path, trashName, onTrash, onOpen }: { path: string; trashName: string; onTrash: TrashFn; onOpen: (entry: FileEntry) => void }) {
  const [target, setTarget] = useState(() => cachedDupeTarget || path);
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(() => cachedDupeGroups);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const run = (folder: string) => {
    setTarget(folder);
    cachedDupeTarget = folder;
    setBusy(true);
    setError("");
    setGroups(null);
    cachedDupeGroups = null;
    api.findDuplicates(folder)
      .then(res => {
        setGroups(res);
        cachedDupeGroups = res;
      })
      .catch(e => setError(errorText(e)))
      .finally(() => setBusy(false));
  };

  useEffect(() => {
    if (!cachedDupeGroups) run(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const keepNewest = async (group: DuplicateGroup) => {
    const extra = [...group.files].sort((a, b) => b.modified - a.modified).slice(1);
    const removed = await onTrash(extra.map(f => f.path), `${extra.length} duplicate${extra.length === 1 ? "" : "s"} of “${group.files[0].name}”`);
    if (removed.length > 0) {
      const removedSet = new Set(removed);
      setGroups(old => {
        if (!old) return null;
        const next = old
          .map(g => g.hash !== group.hash ? g : { ...g, files: g.files.filter(f => !removedSet.has(f.path)) })
          .filter(g => g.files.length > 1);
        cachedDupeGroups = next;
        return next;
      });
    }
  };

  const rows = useMemo<FlatDupeRow[]>(() => {
    if (!groups) return [];
    const flat: FlatDupeRow[] = [];
    for (const group of groups) {
      flat.push({ kind: "header", group });
      const newest = [...group.files].sort((a, b) => b.modified - a.modified)[0];
      for (const file of group.files) {
        flat.push({ kind: "file", group, file, isNewest: file.path === newest.path });
      }
    }
    return flat;
  }, [groups]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: i => (rows[i]?.kind === "header" ? 44 : 52),
    overscan: 12,
  });

  const recoverable = groups?.reduce((sum, g) => sum + g.size * (g.files.length - 1), 0) ?? 0;

  return (
    <div className="tool" style={{ display: "flex", flexDirection: "column", height: "100%", overflow: "hidden" }}>
      <header className="tool-head">
        <div>
          <div className="eyebrow truncate">Duplicates · {target}</div>
          <h1 className="big">{groups ? fmtBytes(recoverable) : busy ? "Scanning…" : "—"}</h1>
          <p>{groups ? `${groups.length} group${groups.length === 1 ? "" : "s"} of identical files could be cleaned up` : "Comparing 4 KiB head, 32 KiB sample, then full BLAKE3 hash"}</p>
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
      {rows.length > 0 && (
        <div ref={scrollRef} className="panel" style={{ flex: 1, overflow: "auto", position: "relative" }}>
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map(vRow => {
              const row = rows[vRow.index];
              const style: React.CSSProperties = {
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${vRow.start}px)`,
              };
              if (row.kind === "header") {
                return (
                  <h2 key={`h-${row.group.hash}`} style={{ ...style, margin: 0, borderTop: vRow.index > 0 ? "1px solid var(--border)" : "none" }}>
                    <span><Copy size={15} style={{ verticalAlign: "-2px", marginRight: 8 }} aria-hidden="true" />{row.group.files.length} copies · {fmtBytes(row.group.size)} each</span>
                    <button type="button" className="btn btn-sm" onClick={() => void keepNewest(row.group)}>Keep newest</button>
                  </h2>
                );
              }
              return (
                <button key={`f-${row.file.path}`} type="button" className="panel-row clickable" style={style} onDoubleClick={() => onOpen(row.file)} title={row.file.path}>
                  <FileIcon entry={row.file} />
                  <span className="grow"><span className="truncate" style={{ display: "block" }}>{row.file.name}</span><span className="sub truncate">{row.file.path}</span></span>
                  <span className="muted small">{fmtDate(row.file.modified)}</span>
                  {row.isNewest && <span className="pill live">Newest</span>}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

const BADGE_LABEL: Record<SafetyBadge, { text: string; tone: string }> = {
  safe: { text: "Safe", tone: "live" },
  sessionReset: { text: "Session Reset", tone: "" },
  appOpen: { text: "App Open", tone: "" },
  permanent: { text: "Permanent", tone: "" },
};

export function CleanupView({ trashName }: { trashName: string; onTrash?: TrashFn }) {
  const [rules, setRules] = useState<CleanerRuleSummary[] | null>(() => cachedDeepRules);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Record<string, CleanerFileItem[]>>({});
  const [openCats, setOpenCats] = useState<Record<string, boolean>>({ System: true, Browsers: true, Applications: true, "Privacy Traces": true });
  const [filter, setFilter] = useState("");
  const [permanent, setPermanent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [error, setError] = useState("");
  const [statusMsg, setStatusMsg] = useState("");

  const scan = (force = false) => {
    setBusy(true);
    setError("");
    setStatusMsg("");
    api.scanDeepCleanup(force)
      .then(res => {
        setRules(res);
        cachedDeepRules = res;
        setSelected(new Set(res.filter(r => r.itemCount > 0 && !r.appRunning && r.badge === "safe").map(r => r.id)));
      })
      .catch(e => setError(errorText(e)))
      .finally(() => setBusy(false));
  };

  useEffect(() => {
    if (!cachedDeepRules) scan(false);
  }, []);

  const toggleExpand = async (rule: CleanerRuleSummary) => {
    if (expanded[rule.id]) {
      setExpanded(old => { const next = { ...old }; delete next[rule.id]; return next; });
      return;
    }
    try {
      const items = await api.getCleanerItems(rule.id, 0, 100);
      setExpanded(old => ({ ...old, [rule.id]: items }));
    } catch { /* ignore */ }
  };

  const filtered = useMemo(() => {
    if (!rules) return [];
    const q = filter.trim().toLowerCase();
    return rules.filter(r => !q || r.name.toLowerCase().includes(q) || r.category.toLowerCase().includes(q) || r.description.toLowerCase().includes(q));
  }, [rules, filter]);

  const grouped = useMemo(() => {
    const map = new Map<string, CleanerRuleSummary[]>();
    for (const r of filtered) {
      const list = map.get(r.category) ?? [];
      list.push(r);
      map.set(r.category, list);
    }
    return [...map.entries()];
  }, [filtered]);

  const totalBytes = rules?.reduce((s, r) => s + r.totalBytes, 0) ?? 0;
  const selectedRules = rules?.filter(r => selected.has(r.id) && r.itemCount > 0) ?? [];
  const selectedBytes = selectedRules.reduce((s, r) => s + r.totalBytes, 0);
  const sqliteTargets = useMemo(() => (rules ?? []).flatMap(r => r.sqliteTargets), [rules]);

  const runClean = async () => {
    if (!selectedRules.length) return;
    setCleaning(true);
    setError("");
    try {
      const res = await api.cleanRules(selectedRules.map(r => r.id), permanent);
      setStatusMsg(`Cleaned ${res.trashed} item${res.trashed === 1 ? "" : "s"}${res.failed.length ? ` · ${res.failed.length} skipped` : ""}`);
      scan(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setCleaning(false);
    }
  };

  const vacuumDbs = async () => {
    if (!sqliteTargets.length) return;
    setCleaning(true);
    try {
      const reclaimed = await api.vacuumSqliteTargets(sqliteTargets);
      setStatusMsg(`Optimized ${sqliteTargets.length} browser database${sqliteTargets.length === 1 ? "" : "s"} · Reclaimed ${fmtBytes(reclaimed)}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setCleaning(false);
    }
  };

  const emptyTrash = async () => {
    setCleaning(true);
    try {
      await api.emptyOsTrash();
      setStatusMsg(`Emptied ${trashName}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setCleaning(false);
    }
  };

  return (
    <div className="tool" style={{ display: "flex", flexDirection: "column", height: "100%", overflow: "hidden" }}>
      <header className="tool-head">
        <div>
          <div className="eyebrow">Deep Cleanup · System, Browsers, Apps & Privacy</div>
          <h1 className="big">{rules ? fmtBytes(totalBytes) : busy ? "Scanning…" : "—"}</h1>
          <p>{statusMsg || (rules ? `${rules.reduce((s, r) => s + r.itemCount, 0).toLocaleString()} cleanable items across ${grouped.length} categories` : "Scanning local caches and temporary files")}</p>
        </div>
        <div className="actions">
          {sqliteTargets.length > 0 && (
            <button type="button" className="btn" disabled={cleaning} onClick={() => void vacuumDbs()}><Database size={15} /> Vacuum DBs ({sqliteTargets.length})</button>
          )}
          <button type="button" className="btn" disabled={cleaning} onClick={() => void emptyTrash()}><Trash2 size={15} /> Empty {trashName}</button>
          <button type="button" className="btn btn-dark" onClick={() => scan(true)} disabled={busy}>{busy ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Rescan</button>
        </div>
      </header>

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10 }}>
        <label className="filter-box" style={{ flex: 1 }}>
          <Search size={14} aria-hidden="true" />
          <input value={filter} onChange={e => setFilter(e.target.value)} placeholder="Filter cleaners (Chrome, Temp, VS Code…)" />
        </label>
        <div className="notice" style={{ margin: 0, padding: "6px 10px" }}>
          <ShieldCheck size={15} />
          <span>{permanent ? "Permanent deletion enabled" : `Recoverable via ${trashName}`}</span>
        </div>
      </div>

      {error && <div className="state error"><h3>Cleanup error</h3><p>{error}</p></div>}

      <div style={{ flex: 1, overflow: "auto", display: "flex", flexDirection: "column", gap: 10, paddingBottom: 12 }}>
        {rules && filtered.length === 0 && <div className="state"><span className="badge"><Sparkles size={20} /></span><h3>Nothing to clean</h3><p>No matching cleanup items found.</p></div>}
        {grouped.map(([category, catRules]) => {
          const isOpen = openCats[category] ?? true;
          const catBytes = catRules.reduce((s, r) => s + r.totalBytes, 0);
          return (
            <section key={category} className="panel">
              <h2 style={{ cursor: "pointer" }} onClick={() => setOpenCats(o => ({ ...o, [category]: !isOpen }))}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                  {isOpen ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
                  {category}
                </span>
                <span className="num">{fmtBytes(catBytes)}</span>
              </h2>
              {isOpen && catRules.map(rule => {
                const badge = BADGE_LABEL[rule.badge];
                const items = expanded[rule.id];
                return (
                  <div key={rule.id} style={{ borderTop: "1px solid var(--border)" }}>
                    <div className="check-row" style={{ alignItems: "center" }}>
                      <input
                        type="checkbox"
                        disabled={rule.itemCount === 0}
                        checked={selected.has(rule.id)}
                        onChange={e => setSelected(old => {
                          const next = new Set(old);
                          if (e.target.checked) next.add(rule.id); else next.delete(rule.id);
                          return next;
                        })}
                      />
                      <span className="grow" style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <strong>{rule.name}</strong>
                          <span className={`pill ${badge.tone}`}>{badge.text}</span>
                          {rule.appRunning && <span className="muted small" style={{ display: "inline-flex", alignItems: "center", gap: 3 }}><AlertTriangle size={12} /> {rule.runningProcess} running</span>}
                        </span>
                        <span className="sub muted small">{rule.description} · {rule.itemCount.toLocaleString()} items</span>
                      </span>
                      <span className="num">{fmtBytes(rule.totalBytes)}</span>
                      {rule.itemCount > 0 && (
                        <button type="button" className="btn btn-sm" onClick={() => void toggleExpand(rule)}>
                          {items ? "Hide" : "Preview"}
                        </button>
                      )}
                    </div>
                    {items && (
                      <div style={{ padding: "4px 14px 10px 38px", background: "var(--bg-subtle, transparent)", fontSize: 12 }}>
                        {items.slice(0, 25).map(it => (
                          <div key={it.path} style={{ display: "flex", justifyContent: "space-between", gap: 8, padding: "2px 0" }} title={it.path}>
                            <span className="truncate muted">{it.name}</span>
                            <span className="num">{fmtBytes(it.bytes)}</span>
                          </div>
                        ))}
                        {rule.itemCount > 25 && <div className="muted">…and {(rule.itemCount - 25).toLocaleString()} more items</div>}
                      </div>
                    )}
                  </div>
                );
              })}
            </section>
          );
        })}
      </div>

      <footer className="panel" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "10px 14px", marginTop: 8 }}>
        <label style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 13 }}>
          <input type="checkbox" checked={permanent} onChange={e => setPermanent(e.target.checked)} />
          <span>Permanent delete (skip {trashName})</span>
        </label>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span className="muted small">{selectedRules.length} group{selectedRules.length === 1 ? "" : "s"} · {fmtBytes(selectedBytes)}</span>
          <button type="button" className={`btn ${permanent ? "danger" : "btn-dark"}`} disabled={!selectedRules.length || cleaning} onClick={() => void runClean()}>
            {cleaning ? <LoaderCircle size={15} className="spin" /> : <Sparkles size={15} />} {permanent ? "Delete Permanently" : `Clean to ${trashName}`}
          </button>
        </div>
      </footer>
    </div>
  );
}
