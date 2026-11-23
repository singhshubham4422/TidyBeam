import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertTriangle, ArrowDown, ArrowUp, FolderOpen, LoaderCircle } from "lucide-react";
import { api, assetUrl, errorText, fmtBytes, fmtDate, IMAGE_EXTENSIONS, type FileEntry, type SortKey } from "../api";
import { FileIcon } from "./FileIcon";

const PAGE = 200;
const TILE_WIDTH = 120;
const THUMB_MAX_BYTES = 40 * 1024 * 1024;

export type Selection = Map<string, FileEntry>;

type Props = {
  path: string;
  showHidden: boolean;
  sort: SortKey;
  descending: boolean;
  filter: string;
  view: "list" | "grid";
  refreshTick: number;
  selection: Selection;
  cutPaths: Set<string>;
  onSelectionChange: (selection: Selection) => void;
  onOpen: (entry: FileEntry) => void;
  onSort?: (key: SortKey) => void;
  onStatus?: (status: { total: number | null; error: string | null; loading: boolean }) => void;
  onFocusPane?: () => void;
  emptyHint?: string;
};

type Data = { key: string; total: number | null; pages: Record<number, FileEntry[]>; error: string | null };

export function FileList(props: Props) {
  const { path, showHidden, sort, descending, filter, view, refreshTick, selection, onSelectionChange, onOpen, onStatus } = props;
  const queryKey = `${path}\u0000${showHidden}\u0000${sort}\u0000${descending}\u0000${filter}`;
  const [data, setData] = useState<Data>({ key: "", total: null, pages: {}, error: null });
  const epoch = useRef(0);
  const inflight = useRef(new Set<number>());
  const lastTick = useRef(refreshTick);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const anchor = useRef<number | null>(null);
  const [focusIndex, setFocusIndex] = useState(0);

  const fetchPage = useCallback(async (page: number, force: boolean, currentEpoch: number) => {
    if (inflight.current.has(page)) return;
    inflight.current.add(page);
    try {
      const result = await api.listEntries({ path, offset: page * PAGE, limit: PAGE, showHidden, sort, descending, filter, force });
      if (currentEpoch !== epoch.current) return;
      setData(old => ({ key: queryKey, total: result.total, error: null, pages: { ...(old.key === queryKey ? old.pages : {}), [page]: result.entries } }));
    } catch (error) {
      if (currentEpoch === epoch.current) setData({ key: queryKey, total: 0, pages: {}, error: errorText(error) });
    } finally {
      inflight.current.delete(page);
    }
  }, [path, showHidden, sort, descending, filter, queryKey]);

  // Reload when the query changes, or refresh in place when asked.
  useEffect(() => {
    const forced = lastTick.current !== refreshTick;
    lastTick.current = refreshTick;
    const current = ++epoch.current;
    inflight.current.clear();
    setData(old => (old.key === queryKey ? { ...old, pages: forced ? old.pages : {} } : { key: queryKey, total: null, pages: {}, error: null }));
    if (!forced) {
      anchor.current = null;
      setFocusIndex(0);
      scrollRef.current?.scrollTo({ top: 0 });
    }
    void fetchPage(0, forced, current).then(() => {
      // Pages other than the first are refetched lazily after a refresh.
      if (forced && current === epoch.current) setData(old => ({ ...old, pages: { 0: old.pages[0] ?? [] } }));
    });
  }, [queryKey, refreshTick, fetchPage]);

  const total = data.key === queryKey ? data.total : null;
  const loading = total === null;
  useEffect(() => { onStatus?.({ total, error: data.error, loading }); }, [total, data.error, loading, onStatus]);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([item]) => setWidth(item.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const columns = view === "grid" ? Math.max(1, Math.floor((width - 16) / TILE_WIDTH)) : 1;
  const count = total ?? 0;
  const rowCount = Math.ceil(count / columns);
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => (view === "grid" ? 148 : parseInt(getComputedStyle(document.documentElement).getPropertyValue("--row")) || 40),
    overscan: 8,
  });
  useEffect(() => { virtualizer.measure(); }, [view, columns, virtualizer]);
  const virtualRows = virtualizer.getVirtualItems();

  const entryAt = useCallback((index: number) => data.pages[Math.floor(index / PAGE)]?.[index % PAGE], [data.pages]);

  // Fetch any page the viewport needs.
  useEffect(() => {
    if (loading || !virtualRows.length) return;
    const first = virtualRows[0].index * columns;
    const last = Math.min(count - 1, (virtualRows[virtualRows.length - 1].index + 1) * columns - 1);
    for (let page = Math.floor(first / PAGE); page <= Math.floor(last / PAGE); page++) {
      if (!data.pages[page]) void fetchPage(page, false, epoch.current);
    }
  }, [virtualRows, columns, count, loading, data.pages, fetchPage]);

  const select = useCallback((index: number, mode: "single" | "toggle" | "range") => {
    const entry = entryAt(index);
    if (!entry) return;
    setFocusIndex(index);
    if (mode === "toggle") {
      const next = new Map(selection);
      if (next.has(entry.path)) next.delete(entry.path); else next.set(entry.path, entry);
      anchor.current = index;
      onSelectionChange(next);
    } else if (mode === "range" && anchor.current !== null) {
      const [from, to] = [Math.min(anchor.current, index), Math.max(anchor.current, index)];
      const next = new Map<string, FileEntry>();
      for (let i = from; i <= to; i++) {
        const item = entryAt(i);
        if (item) next.set(item.path, item);
      }
      onSelectionChange(next);
    } else {
      anchor.current = index;
      onSelectionChange(new Map([[entry.path, entry]]));
    }
  }, [entryAt, selection, onSelectionChange]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (!count) return;
    const step = { ArrowDown: columns, ArrowUp: -columns, ArrowRight: view === "grid" ? 1 : 0, ArrowLeft: view === "grid" ? -1 : 0, PageDown: columns * 10, PageUp: -columns * 10 }[event.key];
    if (step !== undefined && step !== 0) {
      event.preventDefault();
      const next = Math.max(0, Math.min(count - 1, focusIndex + step));
      select(next, event.shiftKey ? "range" : "single");
      virtualizer.scrollToIndex(Math.floor(next / columns));
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const next = event.key === "Home" ? 0 : count - 1;
      select(next, event.shiftKey ? "range" : "single");
      virtualizer.scrollToIndex(Math.floor(next / columns));
    } else if (event.key === "Enter") {
      const entry = entryAt(focusIndex);
      if (entry) { event.preventDefault(); onOpen(entry); }
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
      event.preventDefault();
      const next = new Map<string, FileEntry>();
      Object.values(data.pages).flat().forEach(item => next.set(item.path, item));
      onSelectionChange(next);
    } else if (event.key === "Escape" && selection.size) {
      onSelectionChange(new Map());
    }
  };

  const clickHandler = (index: number) => (event: React.MouseEvent) => {
    props.onFocusPane?.();
    select(index, event.shiftKey ? "range" : event.ctrlKey || event.metaKey ? "toggle" : "single");
  };

  const sortButton = (key: SortKey, label: string, className = "") => (
    <button type="button" className={`${className} ${sort === key ? "sorted" : ""}`} onClick={() => props.onSort?.(key)} aria-label={`Sort by ${label}`}>
      {label}{sort === key && (descending ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}
    </button>
  );

  const body = useMemo(() => {
    if (data.error && data.key === queryKey) {
      return <div className="state error"><span className="badge"><AlertTriangle size={20} /></span><h3>Can't open this folder</h3><p>{data.error}</p></div>;
    }
    if (loading) return <div className="state"><LoaderCircle className="spin" size={20} /><span>Reading folder…</span></div>;
    if (!count) {
      return <div className="state"><span className="badge"><FolderOpen size={20} /></span><h3>{filter ? "No matches" : "This folder is empty"}</h3><p>{filter ? `Nothing here matches “${filter}”.` : props.emptyHint ?? "Create a folder or paste items here."}</p></div>;
    }
    return null;
  }, [data.error, data.key, queryKey, loading, count, filter, props.emptyHint]);

  return (
    <>
      {view === "list" && (
        <div className="list-head" role="presentation">
          {sortButton("name", "Name")}
          {sortButton("size", "Size", "right")}
          {sortButton("modified", "Modified", "col-date")}
        </div>
      )}
      <div
        ref={scrollRef}
        className="scroller"
        tabIndex={0}
        role="listbox"
        aria-multiselectable="true"
        aria-label="Folder contents"
        onKeyDown={onKeyDown}
        onFocus={props.onFocusPane}
        onMouseDown={event => { if (event.target === event.currentTarget) onSelectionChange(new Map()); }}
      >
        {body ?? (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualRows.map(row => {
              if (view === "list") {
                const entry = entryAt(row.index);
                const style = { transform: `translateY(${row.start}px)` };
                if (!entry) return <div key={row.key} className="row" style={style} aria-hidden="true"><span className="muted">…</span></div>;
                const selected = selection.has(entry.path);
                return (
                  <div
                    key={entry.path}
                    className={`row ${selected ? "selected" : ""} ${props.cutPaths.has(entry.path) ? "cut" : ""}`}
                    style={style}
                    role="option"
                    aria-selected={selected}
                    onClick={clickHandler(row.index)}
                    onDoubleClick={() => onOpen(entry)}
                    title={entry.name}
                  >
                    <span className="name-cell"><FileIcon entry={entry} /><span className="truncate">{entry.name}</span>{entry.hidden && <span className="tag">Hidden</span>}</span>
                    <span className="size">{entry.is_dir ? "—" : fmtBytes(entry.size)}</span>
                    <span className="date">{fmtDate(entry.modified)}</span>
                  </div>
                );
              }
              const start = row.index * columns;
              return (
                <div key={row.key} className="grid-row" style={{ transform: `translateY(${row.start}px)`, gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}>
                  {Array.from({ length: Math.min(columns, count - start) }, (_, offset) => {
                    const index = start + offset;
                    const entry = entryAt(index);
                    if (!entry) return <div key={index} className="tile" aria-hidden="true" />;
                    const selected = selection.has(entry.path);
                    const thumb = !entry.is_dir && IMAGE_EXTENSIONS.has(entry.extension) && entry.size < THUMB_MAX_BYTES;
                    return (
                      <div key={entry.path} className={`tile ${selected ? "selected" : ""} ${props.cutPaths.has(entry.path) ? "cut" : ""}`} role="option" aria-selected={selected} title={entry.name} onClick={clickHandler(index)} onDoubleClick={() => onOpen(entry)}>
                        <span className="thumb">{thumb ? <img src={assetUrl(entry.path)} alt="" loading="lazy" decoding="async" draggable={false} onError={event => { event.currentTarget.style.display = "none"; }} /> : <FileIcon entry={entry} size={32} />}</span>
                        <span className="label">{entry.name}</span>
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
