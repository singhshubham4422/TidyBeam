import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BarChart3, Folder, LoaderCircle, RefreshCw } from "lucide-react";
import { api, errorText, fmtBytes, type Analysis, type AnalysisProgress, type FileEntry } from "../api";
import { FileIcon } from "../components/FileIcon";

type Props = { path: string; onNavigate: (path: string) => void; onOpen: (entry: FileEntry) => void; onDuplicates: () => void };

const COLORS = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)"];

export function StorageView({ path, onNavigate, onOpen, onDuplicates }: Props) {
  const [target, setTarget] = useState(path);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [progress, setProgress] = useState<AnalysisProgress | null>(null);
  const [error, setError] = useState("");
  const scanId = useRef("");

  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen<AnalysisProgress>("analysis-progress", event => { if (event.payload.scanId === scanId.current) setProgress(event.payload); }).then(fn => { stop = fn; });
    return () => stop?.();
  }, []);

  const run = (folder: string) => {
    const id = crypto.randomUUID();
    scanId.current = id;
    setTarget(folder); setAnalysis(null); setError(""); setProgress({ scanId: id, filesScanned: 0, foldersScanned: 0 });
    api.analyzeFolder(folder, id)
      .then(result => { if (scanId.current === id) setAnalysis(result); })
      .catch(e => { if (scanId.current === id) setError(errorText(e)); })
      .finally(() => { if (scanId.current === id) setProgress(null); });
  };
  useEffect(() => { run(path); }, [path]);

  const scanning = progress !== null && !analysis;
  const top = analysis?.folders.slice(0, 4) ?? [];
  const rest = analysis ? analysis.total_bytes - top.reduce((sum, f) => sum + f.bytes, 0) : 0;
  const maxFolder = Math.max(1, analysis?.folders[0]?.bytes ?? 1);

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow truncate">Storage · {target}</div>
          <h2 className="big">{analysis ? fmtBytes(analysis.total_bytes) : scanning ? "Scanning…" : "—"}</h2>
          <p>{analysis ? `${analysis.file_count.toLocaleString()} files in ${analysis.folder_count.toLocaleString()} folders` : progress ? `${progress.filesScanned.toLocaleString()} files · ${progress.foldersScanned.toLocaleString()} folders so far` : " "}</p>
        </div>
        <div className="actions">
          <button type="button" className="btn" onClick={() => run(path)} disabled={scanning}>Use current folder</button>
          <button type="button" className="btn btn-dark" onClick={() => run(target)} disabled={scanning}>{scanning ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Rescan</button>
        </div>
      </header>

      {error && <div className="state error"><span className="badge"><BarChart3 size={20} /></span><h3>Couldn't analyze this folder</h3><p>{error}</p></div>}
      {scanning && !error && <div className="state"><LoaderCircle size={22} className="spin" /><span>Measuring folders…</span></div>}

      {analysis && (
        <>
          <section aria-label="Breakdown">
            <div className="stack-bar" role="img" aria-label={`Space by folder: ${top.map(f => `${f.name} ${Math.round((f.bytes / Math.max(analysis.total_bytes, 1)) * 100)}%`).join(", ")}`}>
              {top.map((folder, i) => <span key={folder.path} style={{ flex: Math.max(folder.bytes, 1), background: COLORS[i] }} />)}
              {rest > 0 && <span style={{ flex: rest, background: "var(--chart-5)" }} />}
            </div>
            <div className="legend">
              {top.map((folder, i) => <span key={folder.path}><i style={{ background: COLORS[i] }} />{folder.name}</span>)}
              {rest > 0 && <span><i style={{ background: "var(--chart-5)" }} />Everything else</span>}
            </div>
          </section>
          <div className="columns">
            <section className="panel" aria-labelledby="by-folder" style={{ flexGrow: 3 }}>
              <h2 id="by-folder">By folder</h2>
              {analysis.folders.length === 0 && <div className="state"><p>No items at this level.</p></div>}
              {analysis.folders.slice(0, 40).map(folder => (
                <button key={folder.path} type="button" className="panel-row clickable" onClick={() => folder.is_dir ? run(folder.path) : undefined} onDoubleClick={() => folder.is_dir && onNavigate(folder.path)} title={folder.is_dir ? "Click to drill in, double-click to open" : folder.path}>
                  {folder.is_dir ? <Folder size={18} className="icon-folder" aria-hidden="true" /> : <FileIcon entry={{ name: folder.name, path: folder.path, is_dir: false, is_symlink: false, size: folder.bytes, modified: 0, extension: folder.name.split(".").pop()?.toLowerCase() ?? "", hidden: false, executable: false }} />}
                  <span className="grow"><span className="truncate" style={{ display: "block" }}>{folder.name}</span><span className="sub">{folder.is_dir ? `${folder.files.toLocaleString()} files` : "File"}</span></span>
                  <span className="bar" aria-hidden="true"><i style={{ width: `${Math.max(1, (folder.bytes / maxFolder) * 100)}%` }} /></span>
                  <span className="num" style={{ width: 80, textAlign: "right" }}>{fmtBytes(folder.bytes)}</span>
                </button>
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
        </>
      )}
    </div>
  );
}
