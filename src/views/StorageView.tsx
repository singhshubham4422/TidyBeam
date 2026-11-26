import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BarChart3, ChevronRight, Folder, FolderOpen, LayoutGrid, List, LoaderCircle, RefreshCw, Trash2, XCircle } from "lucide-react";
import { api, errorText, fmtBytes, type Analysis, type AnalysisProgress, type FileEntry, type FolderSize } from "../api";
import { FileIcon } from "../components/FileIcon";

type TrashFn = (paths: string[], what: string) => Promise<string[]>;
type Props = {
  path: string;
  trashName: string;
  onNavigate: (path: string) => void;
  onOpen: (entry: FileEntry) => void;
  onTrash: TrashFn;
  onDuplicates: () => void;
};

const COLORS = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)", "var(--chart-5)"];

let lastStorageTarget = "";
let lastStorageAnalysis: Analysis | null = null;

function formatAge(scannedAt: number): string {
  if (!scannedAt) return "just now";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - scannedAt);
  if (diff < 10) return "just now";
  if (diff < 60) return `${diff}s ago`;
  const mins = Math.floor(diff / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

export function StorageView({ path, trashName, onNavigate, onOpen, onTrash, onDuplicates }: Props) {
  const [target, setTarget] = useState(() => lastStorageTarget || path);
  const [analysis, setAnalysis] = useState<Analysis | null>(() => lastStorageAnalysis);
  const [progress, setProgress] = useState<AnalysisProgress | null>(null);
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"bars" | "treemap">("bars");
  const [sizeMetric, setSizeMetric] = useState<"logical" | "disk">("logical");
  const scanId = useRef("");

  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen<AnalysisProgress>("analysis-progress", event => {
      if (event.payload.scanId === scanId.current) setProgress(event.payload);
    }).then(fn => { stop = fn; });
    return () => stop?.();
  }, []);

  const run = useCallback((folder: string, force = false) => {
    const id = crypto.randomUUID();
    if (scanId.current) void api.cancelAnalysis(scanId.current).catch(() => undefined);
    scanId.current = id;
    setTarget(folder);
    lastStorageTarget = folder;
    setError("");
    if (force) {
      setAnalysis(null);
      lastStorageAnalysis = null;
    }
    setProgress({ scanId: id, filesScanned: 0, foldersScanned: 0, bytesScanned: 0, topFolders: [] });
    api.analyzeFolder(folder, id, force)
      .then(result => {
        if (scanId.current === id) {
          setAnalysis(result);
          lastStorageAnalysis = result;
        }
      })
      .catch(e => {
        const msg = errorText(e);
        if (scanId.current === id && msg !== "Cancelled") setError(msg);
      })
      .finally(() => {
        if (scanId.current === id) setProgress(null);
      });
  }, []);

  const drillInto = useCallback((folder: string) => {
    setTarget(folder);
    lastStorageTarget = folder;
    setError("");
    void api.getCachedAnalysis(folder).then(cached => {
      if (cached) {
        setAnalysis(cached);
        lastStorageAnalysis = cached;
      } else {
        run(folder, false);
      }
    });
  }, [run]);

  const cancelScan = () => {
    if (!scanId.current) return;
    const id = scanId.current;
    scanId.current = "";
    void api.cancelAnalysis(id).catch(() => undefined);
    setProgress(null);
  };

  // On mount, restore cached analysis if available; only scan automatically if never scanned.
  useEffect(() => {
    if (lastStorageAnalysis) return;
    void api.getCachedAnalysis(target).then(cached => {
      if (cached) {
        setAnalysis(cached);
        lastStorageAnalysis = cached;
      } else {
        run(target, false);
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const scanning = progress !== null;
  const sizeOf = useCallback((f: FolderSize) => (sizeMetric === "disk" ? f.allocated_bytes : f.bytes), [sizeMetric]);
  const totalSize = analysis ? (sizeMetric === "disk" ? analysis.allocated_bytes : analysis.total_bytes) : 0;

  const sortedFolders = useMemo(() => {
    if (!analysis) return [];
    return [...analysis.folders].sort((a, b) => sizeOf(b) - sizeOf(a));
  }, [analysis, sizeOf]);

  const top = sortedFolders.slice(0, 4);
  const rest = analysis ? Math.max(0, totalSize - top.reduce((sum, f) => sum + sizeOf(f), 0)) : 0;
  const maxFolder = Math.max(1, sortedFolders[0] ? sizeOf(sortedFolders[0]) : 1);

  const crumbs = useMemo(() => {
    const parts = target.split(/([\\/])/).filter(Boolean);
    const out: { label: string; path: string }[] = [];
    let acc = "";
    for (const part of parts) {
      acc += part;
      if (part === "\\" || part === "/") {
        if (out.length === 0) out.push({ label: acc, path: acc });
        continue;
      }
      out.push({ label: part, path: acc });
    }
    return out;
  }, [target]);

  const categories = useMemo(() => {
    if (!analysis) return [];
    const c = analysis.categories;
    return [
      { label: "Media", bytes: c.media, color: "var(--chart-1)" },
      { label: "Archives", bytes: c.archives, color: "var(--chart-2)" },
      { label: "Code & Dev", bytes: c.code, color: "var(--chart-3)" },
      { label: "Documents", bytes: c.documents, color: "var(--chart-4)" },
      { label: "App Data", bytes: c.appData, color: "var(--chart-5)" },
      { label: "Other", bytes: c.other, color: "var(--muted)" },
    ].filter(x => x.bytes > 0);
  }, [analysis]);

  const trashFolder = async (folder: FolderSize, event: React.MouseEvent) => {
    event.stopPropagation();
    const removed = await onTrash([folder.path], `“${folder.name}”`);
    if (removed.length > 0) run(target, true);
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div style={{ minWidth: 0 }}>
          <div className="eyebrow" style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
            <span>Storage</span>
            {crumbs.map((c, i) => (
              <span key={c.path} style={{ display: "inline-flex", alignItems: "center", gap: 2 }}>
                <ChevronRight size={12} aria-hidden="true" />
                <button
                  type="button"
                  style={{ border: 0, background: "none", padding: 0, color: i === crumbs.length - 1 ? "var(--fg)" : "inherit", font: "inherit", cursor: "pointer" }}
                  onClick={() => drillInto(c.path)}
                >
                  {c.label}
                </button>
              </span>
            ))}
          </div>
          <h2 className="big">{analysis ? fmtBytes(totalSize) : scanning ? fmtBytes(progress.bytesScanned) : "—"}</h2>
          <p>
            {analysis
              ? `${analysis.file_count.toLocaleString()} files in ${analysis.folder_count.toLocaleString()} folders · Scanned ${formatAge(analysis.scanned_at)}`
              : progress
                ? `${progress.filesScanned.toLocaleString()} files · ${progress.foldersScanned.toLocaleString()} folders so far`
                : " "}
          </p>
        </div>
        <div className="actions">
          <div className="seg" role="group" aria-label="Size metric">
            <button type="button" aria-pressed={sizeMetric === "logical"} onClick={() => setSizeMetric("logical")}>Logical</button>
            <button type="button" aria-pressed={sizeMetric === "disk"} onClick={() => setSizeMetric("disk")}>On-Disk</button>
          </div>
          <div className="seg" role="group" aria-label="Layout">
            <button type="button" aria-pressed={mode === "bars"} aria-label="List layout" onClick={() => setMode("bars")}><List size={15} /></button>
            <button type="button" aria-pressed={mode === "treemap"} aria-label="Treemap layout" onClick={() => setMode("treemap")}><LayoutGrid size={15} /></button>
          </div>
          <button type="button" className="btn" onClick={() => run(path, false)} disabled={scanning}>Use current folder</button>
          {scanning ? (
            <button type="button" className="btn danger" onClick={cancelScan}><XCircle size={15} /> Cancel</button>
          ) : (
            <button type="button" className="btn btn-dark" onClick={() => run(target, true)}><RefreshCw size={15} /> Rescan</button>
          )}
        </div>
      </header>

      {error && <div className="state error"><span className="badge"><BarChart3 size={20} /></span><h3>Couldn't analyze this folder</h3><p>{error}</p></div>}
      {scanning && !analysis && !error && <div className="state"><LoaderCircle size={22} className="spin" /><span>Measuring folders…</span></div>}

      {analysis && (
        <>
          <section aria-label="Breakdown">
            <div className="stack-bar" role="img" aria-label={`Space by folder: ${top.map(f => `${f.name} ${Math.round((sizeOf(f) / Math.max(totalSize, 1)) * 100)}%`).join(", ")}`}>
              {top.map((folder, i) => <span key={folder.path} style={{ flex: Math.max(sizeOf(folder), 1), background: COLORS[i] }} />)}
              {rest > 0 && <span style={{ flex: rest, background: "var(--chart-5)" }} />}
            </div>
            <div className="legend">
              {top.map((folder, i) => <span key={folder.path}><i style={{ background: COLORS[i] }} />{folder.name}</span>)}
              {rest > 0 && <span><i style={{ background: "var(--chart-5)" }} />Everything else</span>}
            </div>
            {categories.length > 0 && (
              <div className="legend" style={{ marginTop: 8 }}>
                {categories.map(cat => (
                  <span key={cat.label} className="pill" style={{ gap: 6 }}>
                    <i style={{ background: cat.color, width: 8, height: 8, borderRadius: 99, display: "inline-block" }} />
                    {cat.label}: {fmtBytes(cat.bytes)}
                  </span>
                ))}
              </div>
            )}
          </section>

          {mode === "treemap" ? (
            <section className="panel" aria-label="Treemap">
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6, padding: 12, minHeight: 280 }}>
                {sortedFolders.slice(0, 24).map((folder, i) => {
                  const pct = Math.max(4, Math.round((sizeOf(folder) / Math.max(totalSize, 1)) * 100));
                  return (
                    <button
                      key={folder.path}
                      type="button"
                      onClick={() => folder.is_dir ? drillInto(folder.path) : undefined}
                      onDoubleClick={() => folder.is_dir && onNavigate(folder.path)}
                      title={`${folder.name} · ${fmtBytes(sizeOf(folder))} (${pct}%)`}
                      style={{
                        flex: `${Math.max(1, pct)} 1 ${Math.max(90, pct * 3)}px`,
                        minHeight: 74,
                        border: "1px solid var(--border)",
                        borderRadius: "var(--radius)",
                        background: COLORS[i % COLORS.length],
                        color: "#fff",
                        padding: "8px 10px",
                        display: "flex",
                        flexDirection: "column",
                        justifyContent: "space-between",
                        textAlign: "left",
                        cursor: folder.is_dir ? "pointer" : "default",
                        overflow: "hidden",
                      }}
                    >
                      <span className="truncate" style={{ fontWeight: 600, fontSize: 13 }}>{folder.name}</span>
                      <span style={{ fontSize: 12, opacity: 0.9 }}>{fmtBytes(sizeOf(folder))} · {pct}%</span>
                    </button>
                  );
                })}
              </div>
            </section>
          ) : (
            <div className="columns">
              <section className="panel" aria-labelledby="by-folder" style={{ flexGrow: 3 }}>
                <h2 id="by-folder">By folder</h2>
                {sortedFolders.length === 0 && <div className="state"><p>No items at this level.</p></div>}
                {sortedFolders.slice(0, 50).map(folder => (
                  <div key={folder.path} className="panel-row clickable" onClick={() => folder.is_dir ? drillInto(folder.path) : undefined} onDoubleClick={() => folder.is_dir && onNavigate(folder.path)} title={folder.is_dir ? "Click to drill in, double-click to open" : folder.path}>
                    {folder.is_dir ? <Folder size={18} className="icon-folder" aria-hidden="true" /> : <FileIcon entry={{ name: folder.name, path: folder.path, is_dir: false, is_symlink: false, size: sizeOf(folder), modified: 0, extension: folder.name.split(".").pop()?.toLowerCase() ?? "", hidden: false, executable: false }} />}
                    <span className="grow"><span className="truncate" style={{ display: "block" }}>{folder.name}</span><span className="sub">{folder.is_dir ? `${folder.files.toLocaleString()} files` : "File"}</span></span>
                    <span className="bar" aria-hidden="true"><i style={{ width: `${Math.max(1, (sizeOf(folder) / maxFolder) * 100)}%` }} /></span>
                    <span className="num" style={{ width: 80, textAlign: "right" }}>{fmtBytes(sizeOf(folder))}</span>
                    {folder.is_dir && (
                      <button type="button" className="icon-btn" aria-label={`Open ${folder.name} in Files`} title="Open in Files" onClick={e => { e.stopPropagation(); onNavigate(folder.path); }}>
                        <FolderOpen size={14} />
                      </button>
                    )}
                    <button type="button" className="icon-btn" aria-label={`Move ${folder.name} to ${trashName}`} title={`Move to ${trashName}`} onClick={e => void trashFolder(folder, e)}>
                      <Trash2 size={14} />
                    </button>
                  </div>
                ))}
              </section>
              <section className="panel" aria-labelledby="largest" style={{ flexGrow: 2 }}>
                <h2 id="largest">Largest files</h2>
                {analysis.largest.map(file => (
                  <button key={file.path} type="button" className="panel-row clickable" onDoubleClick={() => onOpen(file)} title={file.path}>
                    <FileIcon entry={file} />
                    <span className="grow"><span className="truncate" style={{ display: "block" }}>{file.name}</span><span className="sub truncate">{file.path}</span></span>
                    <span className="num">{fmtBytes(file.size)}</span>
                  </button>
                ))}
                <div style={{ padding: "8px 12px 10px" }}><button type="button" className="btn btn-sm" onClick={onDuplicates}>Find duplicates here</button></div>
              </section>
            </div>
          )}
        </>
      )}
    </div>
  );
}
