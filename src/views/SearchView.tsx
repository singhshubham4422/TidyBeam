import { useCallback, useEffect, useState } from "react";
import { Eye, LoaderCircle, Pause, Play, RefreshCw, Search, Trash2 } from "lucide-react";
import { api, errorText, fmtBytes, fmtDate, sameOrDescendant, type FileEntry, type IndexRoot } from "../api";
import type { ConfirmOptions } from "../components/Dialogs";
import { FileIcon } from "../components/FileIcon";

type Props = {
  currentPath: string;
  notify: (kind: "success" | "error", text: string) => void;
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  onOpen: (entry: FileEntry) => void;
  onReveal: (entry: FileEntry) => void;
};

export function SearchView({ currentPath, notify, confirm, onOpen, onReveal }: Props) {
  const [roots, setRoots] = useState<IndexRoot[] | null>(null);
  const [indexing, setIndexing] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [scoped, setScoped] = useState(false);
  const [results, setResults] = useState<FileEntry[] | null>(null);
  const [searching, setSearching] = useState(false);

  const refresh = useCallback(() => { api.listIndexRoots().then(setRoots).catch(e => notify("error", errorText(e))); }, [notify]);
  useEffect(refresh, [refresh]);

  const index = async (path: string) => {
    setIndexing(path);
    try {
      const status = await api.indexFolder(path);
      notify(status.watch_error ? "error" : "success", status.watch_error ? `Indexed ${status.indexed.toLocaleString()} items. ${status.watch_error}` : `Indexed ${status.indexed.toLocaleString()} items · live updates on`);
    } catch (e) { notify("error", errorText(e)); }
    finally { setIndexing(null); refresh(); }
  };
  const remove = async (root: IndexRoot) => {
    if (!(await confirm({ title: "Remove from index?", body: `“${root.root}” will no longer be searchable. No files are deleted.`, confirmLabel: "Remove", danger: true }))) return;
    try { await api.removeIndexRoot(root.root); refresh(); } catch (e) { notify("error", errorText(e)); }
  };
  const toggleWatch = async (root: IndexRoot) => {
    try { await api.setIndexWatch(root.root, !root.watch); refresh(); } catch (e) { notify("error", errorText(e)); refresh(); }
  };

  useEffect(() => {
    if (!query.trim()) { setResults(null); return; }
    let current = true;
    setSearching(true);
    const timer = window.setTimeout(() => {
      api.searchIndex(scoped ? currentPath : null, query, 200)
        .then(found => { if (current) setResults(found); })
        .catch(e => { if (current) { setResults([]); notify("error", errorText(e)); } })
        .finally(() => { if (current) setSearching(false); });
    }, 120);
    return () => { current = false; window.clearTimeout(timer); };
  }, [query, scoped, currentPath, notify]);

  const currentIndexed = roots?.some(r => sameOrDescendant(currentPath, r.root));

  return (
    <div className="tool">
      <header className="tool-head">
        <div><h1>Search</h1><p>Indexed folders are searchable instantly from anywhere with the search box. The index stays on this device.</p></div>
        <button type="button" className="btn btn-primary" disabled={indexing !== null || currentIndexed} onClick={() => void index(currentPath)}>
          {indexing === currentPath ? <LoaderCircle size={15} className="spin" /> : <Search size={15} />} {currentIndexed ? "Current folder is indexed" : "Index current folder"}
        </button>
      </header>

      <section className="panel" aria-labelledby="index-roots">
        <h2 id="index-roots">Indexed folders</h2>
        {roots === null && <div className="state"><LoaderCircle size={18} className="spin" /></div>}
        {roots?.length === 0 && <div className="state"><p>Nothing indexed yet. Open a folder and choose “Index current folder”.</p></div>}
        {roots?.map(root => (
          <div key={root.root} className="panel-row">
            <span className="grow"><span className="truncate" style={{ display: "block" }}>{root.root}</span><span className="sub">{root.indexed.toLocaleString()} items · updated {root.indexedAt ? fmtDate(root.indexedAt) : "never"}</span></span>
            <span className={`pill ${root.watching ? "live" : ""}`}>{root.watching ? "Live" : root.watch ? "Starting" : "Paused"}</span>
            <button type="button" className="icon-btn" aria-label={root.watch ? "Pause live updates" : "Resume live updates"} title={root.watch ? "Pause live updates" : "Resume live updates"} onClick={() => void toggleWatch(root)}>{root.watch ? <Pause size={15} /> : <Play size={15} />}</button>
            <button type="button" className="icon-btn" aria-label="Rebuild index" title="Rebuild index" disabled={indexing !== null} onClick={() => void index(root.root)}>{indexing === root.root ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />}</button>
            <button type="button" className="icon-btn" aria-label="Remove from index" title="Remove from index" onClick={() => void remove(root)}><Trash2 size={15} /></button>
          </div>
        ))}
      </section>

      <section className="panel" aria-label="Search results">
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, padding: 8 }}>
          <label className="filter-box" style={{ flex: "1 1 320px", width: "auto", height: 38 }}>
            {searching ? <LoaderCircle size={16} className="spin" /> : <Search size={16} />}
            <span className="sr-only">Search query</span>
            <input value={query} onChange={e => setQuery(e.target.value)} placeholder="Name, ext:pdf, size:>100mb, modified:week, type:folder" spellCheck={false} />
          </label>
          <div className="seg" role="group" aria-label="Search scope">
            <button type="button" aria-pressed={!scoped} onClick={() => setScoped(false)}>Everywhere</button>
            <button type="button" aria-pressed={scoped} onClick={() => setScoped(true)}>Current folder</button>
          </div>
        </div>
        {results?.length === 0 && <div className="state"><h3>No matches</h3><p>Try a shorter name or a different filter.</p></div>}
        {results?.map(entry => (
          <div key={entry.path} className="panel-row" onDoubleClick={() => onOpen(entry)} title={entry.path}>
            <FileIcon entry={entry} />
            <span className="grow"><span className="truncate" style={{ display: "block" }}>{entry.name}</span><span className="sub truncate">{entry.path}</span></span>
            <span className="muted small">{entry.is_dir ? "Folder" : fmtBytes(entry.size)}</span>
            <button type="button" className="icon-btn" aria-label={`Show ${entry.name} in folder`} onClick={() => onReveal(entry)}><Eye size={15} /></button>
            <button type="button" className="btn btn-sm" onClick={() => onOpen(entry)}>Open</button>
          </div>
        ))}
      </section>
    </div>
  );
}
