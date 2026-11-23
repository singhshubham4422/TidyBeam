import { useEffect, useMemo, useRef, useState } from "react";
import { LoaderCircle, Search } from "lucide-react";
import { api, errorText, fmtBytes, type FileEntry } from "../api";
import { FileIcon } from "./FileIcon";

type Chip = { id: string; label: string; token: string; group: string };
const CHIPS: Chip[] = [
  { id: "files", label: "Files", token: "type:file", group: "type" },
  { id: "folders", label: "Folders", token: "type:folder", group: "type" },
  { id: "today", label: "Today", token: "modified:today", group: "time" },
  { id: "week", label: "Last 7 days", token: "modified:week", group: "time" },
  { id: "big", label: "Over 100 MB", token: "size:>100mb", group: "size" },
];

type Props = { onClose: () => void; onPick: (entry: FileEntry) => void; onReveal: (entry: FileEntry) => void; onManage: () => void; modLabel: string };

export function Palette({ onClose, onPick, onReveal, onManage, modLabel }: Props) {
  const [query, setQuery] = useState("");
  const [chips, setChips] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<FileEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [active, setActive] = useState(0);
  const [roots, setRoots] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    api.listIndexRoots().then(r => setRoots(r.length)).catch(() => setRoots(0));
  }, []);

  const fullQuery = useMemo(() => [query.trim(), ...CHIPS.filter(c => chips.has(c.id)).map(c => c.token)].filter(Boolean).join(" "), [query, chips]);

  useEffect(() => {
    if (!fullQuery) { setResults([]); setBusy(false); setError(""); return; }
    let current = true;
    setBusy(true);
    const timer = window.setTimeout(() => {
      api.searchIndex(null, fullQuery, 60)
        .then(found => { if (current) { setResults(found); setActive(0); setError(""); } })
        .catch(e => { if (current) { setResults([]); setError(errorText(e)); } })
        .finally(() => { if (current) setBusy(false); });
    }, 90);
    return () => { current = false; window.clearTimeout(timer); };
  }, [fullQuery]);

  const ordered = useMemo(() => [...results.filter(r => !r.is_dir), ...results.filter(r => r.is_dir)], [results]);
  const files = ordered.filter(r => !r.is_dir);
  const folders = ordered.filter(r => r.is_dir);

  const toggle = (chip: Chip) => setChips(old => {
    const next = new Set([...old].filter(id => CHIPS.find(c => c.id === id)?.group !== chip.group));
    if (!old.has(chip.id)) next.add(chip.id);
    return next;
  });

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") { event.preventDefault(); onClose(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); setActive(i => Math.min(i + 1, Math.max(ordered.length - 1, 0))); }
    else if (event.key === "ArrowUp") { event.preventDefault(); setActive(i => Math.max(i - 1, 0)); }
    else if (event.key === "Enter" && ordered[active]) {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) onReveal(ordered[active]); else onPick(ordered[active]);
    }
  };

  const row = (entry: FileEntry, index: number) => (
    <button key={entry.path} type="button" className={`result ${index === active ? "active" : ""}`} onMouseMove={() => setActive(index)} onClick={() => onPick(entry)}>
      <FileIcon entry={entry} />
      <span className="grow"><span className="truncate" style={{ display: "block", fontWeight: 500 }}>{entry.name}</span><span className="sub truncate">{entry.path}</span></span>
      <span className="muted small">{entry.is_dir ? "Folder" : fmtBytes(entry.size)}</span>
      {index === active && <kbd>Enter</kbd>}
    </button>
  );

  return (
    <div className="backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="palette" role="dialog" aria-modal="true" aria-label="Search files" onKeyDown={onKeyDown}>
        <div className="palette-input">
          {busy ? <LoaderCircle size={20} className="spin" /> : <Search size={20} />}
          <label htmlFor="palette-query" className="sr-only">Search indexed files</label>
          <input id="palette-query" ref={inputRef} value={query} onChange={e => setQuery(e.target.value)} placeholder="Search files and folders" autoComplete="off" spellCheck={false} />
          <kbd>Esc</kbd>
        </div>
        <div className="chips">
          {CHIPS.map(chip => <button key={chip.id} type="button" className="chip" aria-pressed={chips.has(chip.id)} onClick={() => toggle(chip)}>{chip.label}</button>)}
        </div>
        <div className="palette-results">
          {roots === 0 ? (
            <div className="state"><h3>Nothing indexed yet</h3><p>Index a folder in Search to find files instantly.</p><button type="button" className="btn btn-primary" onClick={onManage}>Open Search</button></div>
          ) : error ? (
            <div className="state error"><h3>Search unavailable</h3><p>{error}</p></div>
          ) : !fullQuery ? (
            <div className="state"><p>Type to search names across your indexed folders.</p></div>
          ) : !busy && !ordered.length ? (
            <div className="state"><h3>No matches</h3><p>Try a shorter name or remove a filter.</p></div>
          ) : (
            <>
              {files.length > 0 && <div className="group-label">Files</div>}
              {files.map((entry, i) => row(entry, i))}
              {folders.length > 0 && <div className="group-label">Folders</div>}
              {folders.map((entry, i) => row(entry, files.length + i))}
            </>
          )}
        </div>
        <div className="palette-foot">
          <span>{roots ? `Searching ${roots} indexed folder${roots === 1 ? "" : "s"} · on this device only` : "Search runs locally on this device"}</span>
          <span>↑ ↓ to move · Enter to open · {modLabel} Enter to reveal · <button type="button" onClick={onManage}>Manage index</button></span>
        </div>
      </section>
    </div>
  );
}
