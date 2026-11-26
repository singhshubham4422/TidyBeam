import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { register as registerShortcut, unregisterAll as unregisterAllShortcuts } from "@tauri-apps/plugin-global-shortcut";
import {
  Activity, Archive, ArrowUp, ChevronLeft, ChevronRight, ClipboardPaste, Columns2, Copy, FolderPlus, LayoutGrid, List,
  LoaderCircle, PanelRight, Pencil, Plus, RefreshCw, Scissors, Search, Star, Trash2, X,
} from "lucide-react";
import {
  api, ARCHIVE_EXTENSIONS, errorText, fmtBytes, type ArchiveEntry, type Drive, type FileEntry, type FolderComparison,
  type PathInfo, type Place, type PlatformInfo, type RuleRunActivity, type SortKey, type TransferConflict, type TransferProgress,
} from "./api";
import { Details } from "./components/Details";
import { useDialogs } from "./components/Dialogs";
import { FileList, type Selection } from "./components/FileList";
import { ArchiveDialog, CompareDialog, ConflictDialog } from "./components/Overlays";
import { Palette } from "./components/Palette";
import { Sidebar, type ToolView } from "./components/Sidebar";
import { isActive, percentOf, Tray, type TransferTask } from "./components/Tray";
import { useAppearance, useSettings } from "./settings";
import { DuplicatesView, CleanupView } from "./views/CleanupViews";
import { RulesView } from "./views/RulesView";
import { SearchView } from "./views/SearchView";
import { SettingsView } from "./views/SettingsView";
import { StorageView } from "./views/StorageView";
import { HealthView, MonitorView, ProcessView, StartupView } from "./views/SystemViews";
import { AuditLogView, CustomCleanersView, DevBloatView, PrivacyShredView } from "./views/Phase4Views";
import { AppsView, OsToolsBackupView } from "./views/Phase5Views";

type Tab = { id: number; history: string[]; index: number };
type Notice = { kind: "success" | "error"; text: string; action?: { label: string; run: () => void } };
type UndoOperation = { kind: "rename"; newPath: string; oldName: string } | { kind: "create-folder"; path: string };
type PendingTransfer = { sources: string[]; destination: string; move: boolean; conflicts: TransferConflict[] };
type Pane = "primary" | "secondary";

const SESSION_KEY = "tidybeam-session-v1";
const TASKS_KEY = "tidybeam-transfers-v1";
const lastName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) || path;
const isEditable = (target: EventTarget | null) => {
  const element = target as HTMLElement | null;
  return !!element && (element.tagName === "INPUT" || element.tagName === "TEXTAREA" || element.tagName === "SELECT" || element.isContentEditable);
};

function loadSession(home: string): { tabs: Tab[]; active: number } {
  try {
    const raw = JSON.parse(localStorage.getItem(SESSION_KEY) || "null") as { paths?: unknown; active?: unknown } | null;
    const paths = Array.isArray(raw?.paths) ? raw.paths.filter((p): p is string => typeof p === "string").slice(0, 12) : [];
    if (paths.length) {
      const tabs = paths.map((path, i) => ({ id: i + 1, history: [path], index: 0 }));
      const active = typeof raw?.active === "number" && raw.active >= 0 && raw.active < tabs.length ? tabs[raw.active].id : 1;
      return { tabs, active };
    }
  } catch { /* fall through */ }
  return { tabs: [{ id: 1, history: [home], index: 0 }], active: 1 };
}

function loadTasks(): Record<string, TransferTask> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(TASKS_KEY) || "{}");
    if (!raw || typeof raw !== "object") return {};
    const valid = Object.entries(raw as Record<string, TransferTask>).filter(([, task]) =>
      task && typeof task === "object" && typeof task.destination === "string" && typeof task.count === "number"
      && task.progress && typeof task.progress.status === "string" && typeof task.progress.completedBytes === "number");
    return Object.fromEntries(valid.slice(-20).map(([id, task]) => [id, isActive(task.progress.status)
      ? { ...task, progress: { ...task.progress, status: "failed" as const, message: "Interrupted when TidyBeam closed" } }
      : task]));
  } catch { return {}; }
}

export default function App() {
  const [settings, update] = useSettings();
  useAppearance(settings);
  const dialogs = useDialogs();
  const [platform, setPlatform] = useState<PlatformInfo | null>(null);
  const [places, setPlaces] = useState<Place[]>([]);
  const [drives, setDrives] = useState<Drive[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTab, setActiveTab] = useState(1);
  const [view, setView] = useState<ToolView>("files");
  const [info, setInfo] = useState<PathInfo | null>(null);
  const [selection, setSelection] = useState<Selection>(new Map());
  const [filterInput, setFilterInput] = useState("");
  const [filter, setFilter] = useState("");
  const [listStatus, setListStatus] = useState<{ total: number | null; error: string | null; loading: boolean }>({ total: null, error: null, loading: true });
  const [refreshTick, setRefreshTick] = useState(0);
  const [editingPath, setEditingPath] = useState(false);
  const [split, setSplit] = useState<{ open: boolean; path: string; selection: Selection }>({ open: false, path: "", selection: new Map() });
  const [focusedPane, setFocusedPane] = useState<Pane>("primary");
  const [clipboard, setClipboard] = useState<{ entries: FileEntry[]; cut: boolean } | null>(null);
  const [tasks, setTasks] = useState<Record<string, TransferTask>>(loadTasks);
  const [trayOpen, setTrayOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [undo, setUndo] = useState<UndoOperation[]>([]);
  const [pending, setPending] = useState<PendingTransfer | null>(null);
  const [archive, setArchive] = useState<{ path: string; entries: ArchiveEntry[]; loading: boolean; destination: string } | null>(null);
  const [comparison, setComparison] = useState<FolderComparison | null>(null);
  const [busy, setBusy] = useState(false);
  const [shortcutError, setShortcutError] = useState("");
  const [startupError, setStartupError] = useState("");
  const toastTimer = useRef<number | undefined>(undefined);

  const tab = tabs.find(t => t.id === activeTab);
  const path = tab ? tab.history[tab.index] : "";
  const os = platform?.os ?? "windows";
  const mod = os === "macos" ? "⌘" : "Ctrl";
  const trashName = platform?.trashName ?? "Trash";
  const revealLabel = os === "macos" ? "Show in Finder" : os === "windows" ? "Show in Explorer" : "Show in folder";
  const samePath = useCallback((a: string, b: string) => {
    const clean = (p: string) => (p.length > 1 ? p.replace(/[\\/]+$/, "") : p);
    return os === "linux" ? clean(a) === clean(b) : clean(a).toLowerCase() === clean(b).toLowerCase();
  }, [os]);

  const notify = useCallback((kind: Notice["kind"], text: string, action?: Notice["action"]) => {
    window.clearTimeout(toastTimer.current);
    setNotice({ kind, text, action });
    toastTimer.current = window.setTimeout(() => setNotice(null), kind === "error" ? 7000 : 4000);
  }, []);
  const refresh = useCallback(() => setRefreshTick(t => t + 1), []);

  // ---------- Startup ----------
  useEffect(() => {
    Promise.all([api.platformInfo(), api.userPlaces(), api.listDrives()])
      .then(([p, pl, d]) => {
        setPlatform(p); setPlaces(pl); setDrives(d);
        const session = loadSession(p.home);
        setTabs(session.tabs); setActiveTab(session.active);
        setSplit(s => ({ ...s, path: p.home }));
      })
      .catch(e => setStartupError(errorText(e)));
  }, []);

  useEffect(() => {
    if (!tabs.length) return;
    try { localStorage.setItem(SESSION_KEY, JSON.stringify({ paths: tabs.map(t => t.history[t.index]), active: tabs.findIndex(t => t.id === activeTab) })); } catch { /* ignore */ }
  }, [tabs, activeTab]);
  useEffect(() => {
    try { localStorage.setItem(TASKS_KEY, JSON.stringify(Object.fromEntries(Object.entries(tasks).slice(-20)))); } catch { /* ignore */ }
  }, [tasks]);

  useEffect(() => {
    if (!path) return;
    let current = true;
    api.pathInfo(path).then(result => { if (current) setInfo(result); }).catch(() => { if (current) setInfo(null); });
    setSelection(new Map());
    return () => { current = false; };
  }, [path]);

  useEffect(() => { const timer = window.setTimeout(() => setFilter(filterInput.trim()), 160); return () => window.clearTimeout(timer); }, [filterInput]);
  useEffect(() => { setFilterInput(""); setFilter(""); }, [path]);

  // Pick up changes made outside the app when the window regains focus.
  const viewRef = useRef(view);
  viewRef.current = view;
  const staleFiles = useRef(false);
  useEffect(() => {
    if (view === "files" && staleFiles.current) {
      staleFiles.current = false;
      refresh();
    }
  }, [view, refresh]);
  useEffect(() => {
    const onFocus = () => {
      if (viewRef.current === "files") refresh();
      else staleFiles.current = true;
      void api.listDrives().then(setDrives).catch(() => undefined);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // ---------- Navigation ----------
  const navigate = useCallback((target: string) => {
    setView("files");
    setEditingPath(false);
    setTabs(old => old.map(t => {
      if (t.id !== activeTab || t.history[t.index] === target) return t;
      const history = [...t.history.slice(0, t.index + 1), target].slice(-100);
      return { ...t, history, index: history.length - 1 };
    }));
  }, [activeTab]);
  const go = (delta: number) => setTabs(old => old.map(t => t.id === activeTab ? { ...t, index: Math.max(0, Math.min(t.history.length - 1, t.index + delta)) } : t));
  const goUp = () => { if (info?.parent) navigate(info.parent); };
  const navigateTyped = async (raw: string) => {
    try { navigate((await api.pathInfo(raw)).path); } catch (e) { notify("error", errorText(e)); }
  };
  const addTab = (target = path || platform?.home || "") => {
    const id = Date.now();
    setTabs(old => [...old, { id, history: [target], index: 0 }]);
    setActiveTab(id); setView("files");
  };
  const closeTab = (id: number) => {
    if (tabs.length === 1) return;
    const index = tabs.findIndex(t => t.id === id);
    const next = tabs.filter(t => t.id !== id);
    if (id === activeTab) setActiveTab(next[Math.max(0, index - 1)].id);
    setTabs(next);
  };

  // ---------- Opening ----------
  const openEntry = useCallback(async (entry: FileEntry, pane: Pane = "primary") => {
    if (entry.is_dir && !(os === "macos" && entry.extension === "app")) {
      if (pane === "secondary") setSplit(s => ({ ...s, path: entry.path, selection: new Map() }));
      else navigate(entry.path);
      return;
    }
    if (ARCHIVE_EXTENSIONS.has(entry.extension)) {
      const destination = pane === "secondary" ? split.path : path;
      setArchive({ path: entry.path, entries: [], loading: true, destination });
      try { const entries = await api.listArchive(entry.path); setArchive(a => a && a.path === entry.path ? { ...a, entries, loading: false } : a); }
      catch (e) { setArchive(null); notify("error", errorText(e)); }
      return;
    }
    try { await api.openItem(entry.path); }
    catch (e) {
      if (errorText(e) !== "confirm-launch") { notify("error", errorText(e)); return; }
      const ok = await dialogs.confirm({
        title: `Run “${entry.name}”?`,
        body: "This is a program or script. Only run it if you trust where it came from.",
        confirmLabel: "Run",
        danger: true,
      });
      if (ok) api.openItem(entry.path, true).catch(err => notify("error", errorText(err)));
    }
  }, [os, navigate, notify, dialogs, path, split.path]);
  const revealEntry = useCallback((entry: FileEntry) => { api.revealItem(entry.path).catch(e => notify("error", errorText(e))); }, [notify]);

  // ---------- Trash ----------
  const trash = useCallback(async (paths: string[], what: string): Promise<string[]> => {
    if (!paths.length) return [];
    if (settings.confirmTrash && !(await dialogs.confirm({ title: `Move ${what} to the ${trashName}?`, body: `You can restore ${paths.length === 1 ? "it" : "them"} from the ${trashName}.`, confirmLabel: `Move to ${trashName}` }))) return [];
    setBusy(true);
    try {
      const result = await api.trashPaths(paths);
      const failedSet = new Set(result.failed.map(f => f.path));
      const removed = paths.filter(p => !failedSet.has(p));
      if (result.failed.length) {
        const ok = await dialogs.confirm({
          title: `${result.failed.length} item${result.failed.length === 1 ? "" : "s"} couldn't go to the ${trashName}`,
          body: <><p>{result.failed[0].error}</p><p><strong>Delete {result.failed.length === 1 ? "it" : "them"} permanently instead? This can't be undone.</strong></p></>,
          confirmLabel: "Delete permanently",
          cancelLabel: "Keep",
          danger: true,
        });
        if (ok) {
          await api.deletePermanently(result.failed.map(f => f.path));
          removed.push(...result.failed.map(f => f.path));
        }
      }
      if (result.trashed) notify("success", `Moved ${result.trashed} item${result.trashed === 1 ? "" : "s"} to the ${trashName}`);
      refresh();
      return removed;
    } catch (e) { notify("error", errorText(e)); refresh(); return []; }
    finally { setBusy(false); }
  }, [settings.confirmTrash, dialogs, trashName, notify, refresh]);

  const activeSelection = focusedPane === "secondary" && split.open ? split.selection : selection;
  const activeFolder = focusedPane === "secondary" && split.open ? split.path : path;
  const selectedEntries = useMemo(() => [...activeSelection.values()], [activeSelection]);
  const describe = (entries: FileEntry[]) => entries.length === 1 ? `“${entries[0].name}”` : `${entries.length} items`;

  // ---------- File operations ----------
  const newFolder = async () => {
    const name = await dialogs.prompt({ title: "New folder", label: "Name", initial: "New folder", confirmLabel: "Create" });
    if (!name) return;
    try {
      const created = await api.createFolder(activeFolder, name);
      setUndo(u => [{ kind: "create-folder" as const, path: created }, ...u].slice(0, 20));
      notify("success", "Folder created", { label: "Undo", run: () => void runUndo() });
      refresh();
    } catch (e) { notify("error", errorText(e)); }
  };
  const rename = async () => {
    const entry = selectedEntries[0];
    if (!entry || selectedEntries.length !== 1) return;
    const name = await dialogs.prompt({ title: "Rename", label: "New name", initial: entry.name, confirmLabel: "Rename", selectStem: !entry.is_dir });
    if (!name || name === entry.name) return;
    try {
      const newPath = await api.renamePath(entry.path, name);
      setUndo(u => [{ kind: "rename" as const, newPath, oldName: entry.name }, ...u].slice(0, 20));
      notify("success", "Renamed", { label: "Undo", run: () => void runUndo() });
      refresh();
    } catch (e) { notify("error", errorText(e)); }
  };
  const undoRef = useRef(undo); undoRef.current = undo;
  const runUndo = async () => {
    const operation = undoRef.current[0];
    if (!operation) return;
    try {
      if (operation.kind === "rename") await api.renamePath(operation.newPath, operation.oldName);
      else await api.undoCreatedFolder(operation.path);
      setUndo(u => u.slice(1));
      notify("success", operation.kind === "rename" ? "Rename undone" : "Folder removed");
      refresh();
    } catch (e) { notify("error", errorText(e)); }
  };
  const zipSelection = async () => {
    if (!selectedEntries.length) return;
    const base = selectedEntries.length === 1 ? selectedEntries[0].name.replace(/\.[^.]+$/, "") : "Archive";
    const name = await dialogs.prompt({ title: "Create ZIP archive", label: "Archive name", initial: `${base}.zip`, confirmLabel: "Create", selectStem: true });
    if (!name) return;
    setBusy(true);
    try { const output = await api.createZip(selectedEntries.map(e => e.path), activeFolder, name); notify("success", `Created ${lastName(output)}`); refresh(); }
    catch (e) { notify("error", errorText(e)); }
    finally { setBusy(false); }
  };
  const toggleFavorite = () => {
    const exists = settings.favorites.some(f => samePath(f, path));
    update({ favorites: exists ? settings.favorites.filter(f => !samePath(f, path)) : [...settings.favorites, path] });
  };

  // ---------- Transfers ----------
  const startTransfer = async (sources: string[], destination: string, move: boolean, conflict: "skip" | "keep-both" | "replace") => {
    const taskId = crypto.randomUUID();
    const progress: TransferProgress = { taskId, status: "queued", completedBytes: 0, totalBytes: 0, completedItems: 0, totalItems: sources.length, currentName: "Preparing…", speedBytesPerSecond: 0 };
    setTasks(old => Object.fromEntries([...Object.entries(old), [taskId, { move, destination, count: sources.length, progress }]].slice(-20)));
    try {
      await api.startTransfer(taskId, sources, destination, move, conflict);
      if (move) setClipboard(old => (old?.cut ? null : old));
    }
    catch (e) { setTasks(old => { const next = { ...old }; delete next[taskId]; return next; }); notify("error", errorText(e)); }
  };
  const beginTransfer = async (sources: string[], destination: string, move: boolean) => {
    if (!sources.length || !destination) return;
    try {
      const preview = await api.previewTransfer(sources, destination);
      if (preview.conflicts.length) setPending({ sources, destination, move, conflicts: preview.conflicts });
      else await startTransfer(sources, destination, move, "skip");
    } catch (e) { notify("error", errorText(e)); }
  };
  const paste = async () => {
    if (!clipboard) return;
    await beginTransfer(clipboard.entries.map(e => e.path), activeFolder, clipboard.cut);
  };
  const cutPaths = useMemo(() => new Set(clipboard?.cut ? clipboard.entries.map(e => e.path) : []), [clipboard]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen<TransferProgress>("transfer-progress", event => {
      const p = event.payload;
      setTasks(old => old[p.taskId] ? { ...old, [p.taskId]: { ...old[p.taskId], progress: p } } : old);
      if (p.status === "completed") { notify("success", `Transfer finished · ${p.message ?? "done"}`); refresh(); }
      else if (p.status === "failed") { notify("error", p.message || "Transfer failed"); refresh(); }
      else if (p.status === "cancelled") { notify("success", "Transfer cancelled"); refresh(); }
    }).then(fn => { stop = fn; });
    return () => stop?.();
  }, [notify, refresh]);
  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen<RuleRunActivity>("organize-rule-run", event => { notify("success", `${event.payload.ruleName}: moved ${event.payload.completed} file${event.payload.completed === 1 ? "" : "s"}`); refresh(); }).then(fn => { stop = fn; });
    return () => stop?.();
  }, [notify, refresh]);

  const control = (id: string, action: "pause" | "resume" | "cancel") => {
    const call = action === "pause" ? api.pauseTransfer : action === "resume" ? api.resumeTransfer : api.cancelTransfer;
    call(id).catch(e => notify("error", errorText(e)));
  };
  const activeTasks = Object.values(tasks).filter(t => isActive(t.progress.status));

  // ---------- Global shortcut (opt-in) ----------
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try { await unregisterAllShortcuts(); } catch { /* nothing registered */ }
      if (!settings.globalShortcutEnabled) { setShortcutError(""); return; }
      try {
        await registerShortcut(settings.globalShortcut, event => {
          if (event.state !== "Pressed") return;
          const win = getCurrentWindow();
          void win.show().then(() => win.setFocus()).catch(() => undefined);
          setPaletteOpen(true);
        });
        if (!cancelled) setShortcutError("");
      } catch (e) { if (!cancelled) setShortcutError(`Couldn't register this shortcut: ${errorText(e)}`); }
    })();
    return () => { cancelled = true; };
  }, [settings.globalShortcutEnabled, settings.globalShortcut]);

  // ---------- Keyboard ----------
  const overlayOpen = dialogs.open || paletteOpen || !!pending || !!archive || !!comparison;
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => undefined);
  keyHandler.current = (event: KeyboardEvent) => {
    const primary = os === "macos" ? event.metaKey : event.ctrlKey;
    const key = event.key.toLowerCase();
    if (primary && key === "k" && !dialogs.open) { event.preventDefault(); setPaletteOpen(open => !open); return; }
    if (overlayOpen) return;
    if (primary && key === "l") { event.preventDefault(); setView("files"); setEditingPath(true); return; }
    if (primary && key === "t") { event.preventDefault(); addTab(); return; }
    if (primary && key === "w" && tabs.length > 1) { event.preventDefault(); closeTab(activeTab); return; }
    if ((event.altKey && event.key === "ArrowLeft") || (os === "macos" && event.metaKey && event.key === "[")) { event.preventDefault(); go(-1); return; }
    if ((event.altKey && event.key === "ArrowRight") || (os === "macos" && event.metaKey && event.key === "]")) { event.preventDefault(); go(1); return; }
    if ((event.altKey && event.key === "ArrowUp") || (os === "macos" && event.metaKey && event.key === "ArrowUp")) { event.preventDefault(); goUp(); return; }
    if (view !== "files" || isEditable(event.target)) return;
    if (primary && key === "c" && selectedEntries.length) { setClipboard({ entries: selectedEntries, cut: false }); notify("success", `Copied ${describe(selectedEntries)}`); return; }
    if (primary && key === "x" && selectedEntries.length) { setClipboard({ entries: selectedEntries, cut: true }); return; }
    if (primary && key === "v" && clipboard) { event.preventDefault(); void paste(); return; }
    if (primary && key === "z" && undo.length) { event.preventDefault(); void runUndo(); return; }
    if (primary && event.shiftKey && key === "n") { event.preventDefault(); void newFolder(); return; }
    if (primary && event.key === "\\") { event.preventDefault(); setSplit(s => ({ ...s, open: !s.open })); return; }
    if (event.key === "F2") { event.preventDefault(); void rename(); return; }
    if (event.key === "F5") { event.preventDefault(); refresh(); return; }
    if ((event.key === "Delete" || (os === "macos" && event.metaKey && event.key === "Backspace")) && selectedEntries.length) {
      event.preventDefault(); void trash(selectedEntries.map(e => e.path), describe(selectedEntries)); return;
    }
    if (event.key === "Backspace" && os !== "macos") { event.preventDefault(); go(-1); }
  };
  useEffect(() => {
    const handler = (event: KeyboardEvent) => keyHandler.current(event);
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  if (startupError) {
    return (
      <main className="crash" role="alert">
        <div className="crash-card">
          <h1>TidyBeam couldn't start</h1>
          <p>{startupError}</p>
          <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>Try again</button>
        </div>
      </main>
    );
  }
  if (!platform || !tab) {
    return <div className="splash"><LoaderCircle className="spin" size={22} aria-label="Loading" /></div>;
  }

  const currentName = info?.segments.at(-1)?.name ?? lastName(path);
  const favorite = settings.favorites.some(f => samePath(f, path));
  const listKey = `${settings.compact}`;
  const hasSelection = selectedEntries.length > 0;

  const fileView = (
    <>
      <div className="page-head">
        <div style={{ minWidth: 0 }}>
          <h1 className="truncate">{currentName}</h1>
          <div className="subtitle">{listStatus.loading ? "Loading…" : listStatus.error ? "Unavailable" : `${(listStatus.total ?? 0).toLocaleString()} item${listStatus.total === 1 ? "" : "s"}${filter ? " match" : ""}`}</div>
        </div>
        <div className="actions">
          <button type="button" className={`icon-btn ${favorite ? "on" : ""}`} aria-pressed={favorite} aria-label={favorite ? "Remove from favorites" : "Add to favorites"} title={favorite ? "Remove from favorites" : "Add to favorites"} onClick={toggleFavorite}><Star size={17} fill={favorite ? "currentColor" : "none"} /></button>
          <button type="button" className="btn" disabled={!clipboard} onClick={() => void paste()}><ClipboardPaste size={15} /> Paste{clipboard ? ` ${clipboard.entries.length}` : ""}</button>
          <button type="button" className="btn btn-primary" onClick={() => void newFolder()}><FolderPlus size={15} /> New folder</button>
        </div>
      </div>
      <div className="toolbar">
        <div className="toolbar-group" role="toolbar" aria-label="Selection actions">
          <button type="button" className="btn" disabled={!hasSelection} aria-label="Copy" onClick={() => setClipboard({ entries: selectedEntries, cut: false })}><Copy size={15} /><span className="btn-label">Copy</span></button>
          <button type="button" className="btn" disabled={!hasSelection} aria-label="Cut" onClick={() => setClipboard({ entries: selectedEntries, cut: true })}><Scissors size={15} /><span className="btn-label">Cut</span></button>
          <button type="button" className="btn" disabled={selectedEntries.length !== 1} onClick={() => void rename()}><Pencil size={15} /><span className="btn-label">Rename</span></button>
          <button type="button" className="btn" disabled={!hasSelection} onClick={() => void zipSelection()}><Archive size={15} /><span className="btn-label">Zip</span></button>
          <span className="divider" />
          <button type="button" className="btn danger" disabled={!hasSelection} onClick={() => void trash(selectedEntries.map(e => e.path), describe(selectedEntries))}><Trash2 size={15} /><span className="btn-label">Move to {trashName}</span></button>
        </div>
        <div className="toolbar-group">
          <label className="filter-box"><Search size={14} aria-hidden="true" /><span className="sr-only">Filter this folder</span><input value={filterInput} onChange={e => setFilterInput(e.target.value)} placeholder="Filter" onKeyDown={e => { if (e.key === "Escape") setFilterInput(""); }} />{filterInput && <button type="button" className="icon-btn" style={{ width: 20, height: 20 }} aria-label="Clear filter" onClick={() => setFilterInput("")}><X size={12} /></button>}</label>
          <select className="select" aria-label="Sort by" value={`${settings.sort}:${settings.descending}`} onChange={e => { const [sort, desc] = e.target.value.split(":"); update({ sort: sort as SortKey, descending: desc === "true" }); }}>
            <option value="name:false">Name A–Z</option>
            <option value="name:true">Name Z–A</option>
            <option value="modified:true">Newest first</option>
            <option value="modified:false">Oldest first</option>
            <option value="size:true">Largest first</option>
            <option value="size:false">Smallest first</option>
            <option value="kind:false">Kind</option>
          </select>
          <div className="seg" role="group" aria-label="View">
            <button type="button" aria-pressed={settings.view === "list"} aria-label="List view" onClick={() => update({ view: "list" })}><List size={15} /></button>
            <button type="button" aria-pressed={settings.view === "grid"} aria-label="Grid view" onClick={() => update({ view: "grid" })}><LayoutGrid size={15} /></button>
          </div>
          <button type="button" className={`icon-btn ${split.open ? "on" : ""}`} aria-pressed={split.open} aria-label="Dual pane" title={`Dual pane (${mod}+\\)`} onClick={() => setSplit(s => ({ ...s, open: !s.open }))}><Columns2 size={17} /></button>
          {!split.open && <button type="button" className={`icon-btn ${settings.showDetails ? "on" : ""}`} aria-pressed={settings.showDetails} aria-label="Details panel" title="Details panel" onClick={() => update({ showDetails: !settings.showDetails })}><PanelRight size={17} /></button>}
        </div>
      </div>
      <div className="browser">
        <section className={`pane ${split.open ? "framed" : ""} ${split.open && focusedPane === "primary" ? "focused" : ""}`} aria-label="Files">
          {split.open && <div className="pane-head"><span className="truncate" style={{ padding: "0 8px", fontWeight: 500 }}>{path}</span></div>}
          <FileList
            key={listKey}
            path={path}
            showHidden={settings.showHidden}
            sort={settings.sort}
            descending={settings.descending}
            filter={filter}
            view={settings.view}
            refreshTick={refreshTick}
            selection={selection}
            cutPaths={cutPaths}
            onSelectionChange={setSelection}
            onOpen={entry => void openEntry(entry, "primary")}
            onSort={key => update(key === settings.sort ? { descending: !settings.descending } : { sort: key, descending: key === "modified" || key === "size" })}
            onStatus={setListStatus}
            onFocusPane={() => setFocusedPane("primary")}
          />
        </section>
        {split.open ? (
          <>
            <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", gap: 6 }}>
              <button type="button" className="btn btn-sm" disabled={!selection.size} title="Copy to right" onClick={() => void beginTransfer([...selection.keys()], split.path, false)}>Copy →</button>
              <button type="button" className="btn btn-sm" disabled={!selection.size} title="Move to right" onClick={() => void beginTransfer([...selection.keys()], split.path, true)}>Move →</button>
              <button type="button" className="btn btn-sm" disabled={!split.selection.size} title="Copy to left" onClick={() => void beginTransfer([...split.selection.keys()], path, false)}>← Copy</button>
              <button type="button" className="btn btn-sm" disabled={!split.selection.size} title="Move to left" onClick={() => void beginTransfer([...split.selection.keys()], path, true)}>← Move</button>
              <button type="button" className="btn btn-sm" title="Compare folders" onClick={() => api.compareFolders(path, split.path, settings.showHidden).then(setComparison).catch(e => notify("error", errorText(e)))}>Compare</button>
            </div>
            <section className={`pane framed ${focusedPane === "secondary" ? "focused" : ""}`} aria-label="Second pane">
              <SecondaryHead path={split.path} onNavigate={target => api.pathInfo(target).then(i => setSplit(s => ({ ...s, path: i.path, selection: new Map() }))).catch(e => notify("error", errorText(e)))} />
              <FileList
                key={listKey}
                path={split.path}
                showHidden={settings.showHidden}
                sort={settings.sort}
                descending={settings.descending}
                filter=""
                view="list"
                refreshTick={refreshTick}
                selection={split.selection}
                cutPaths={cutPaths}
                onSelectionChange={sel => setSplit(s => ({ ...s, selection: sel }))}
                onOpen={entry => void openEntry(entry, "secondary")}
                onFocusPane={() => setFocusedPane("secondary")}
              />
            </section>
          </>
        ) : settings.showDetails && (
          <Details entries={[...selection.values()]} folderName={currentName} revealLabel={revealLabel} onOpen={entry => void openEntry(entry)} onReveal={revealEntry} />
        )}
      </div>
    </>
  );

  return (
    <div className="app">
      <Sidebar
        view={view}
        currentPath={path}
        places={places}
        favorites={settings.favorites}
        drives={drives}
        samePath={samePath}
        onNavigate={navigate}
        onView={setView}
        onRemoveFavorite={target => update({ favorites: settings.favorites.filter(f => f !== target) })}
      />
      <main className="main">
        <div className="tabs" role="tablist" aria-label="Tabs">
          {tabs.map(t => {
            const tabPath = t.history[t.index];
            return (
              <div key={t.id} role="tab" aria-selected={t.id === activeTab} className={`tab ${t.id === activeTab ? "active" : ""}`} title={tabPath}>
                <button type="button" style={{ border: 0, background: "none", padding: 0, color: "inherit" }} className="truncate" onClick={() => { setActiveTab(t.id); setView("files"); }}>{lastName(tabPath)}</button>
                {tabs.length > 1 && <button type="button" className="tab-close" aria-label={`Close ${lastName(tabPath)}`} onClick={() => closeTab(t.id)}><X size={12} /></button>}
              </div>
            );
          })}
          <button type="button" className="icon-btn" aria-label="New tab" title={`New tab (${mod}+T)`} onClick={() => addTab()}><Plus size={15} /></button>
        </div>
        <header className="topbar">
          <div className="nav-buttons">
            <button type="button" className="icon-btn" aria-label="Back" title="Back" disabled={tab.index === 0} onClick={() => go(-1)}><ChevronLeft size={18} /></button>
            <button type="button" className="icon-btn" aria-label="Forward" title="Forward" disabled={tab.index >= tab.history.length - 1} onClick={() => go(1)}><ChevronRight size={18} /></button>
            <button type="button" className="icon-btn" aria-label="Up one folder" title="Up one folder" disabled={!info?.parent} onClick={goUp}><ArrowUp size={17} /></button>
            <button type="button" className="icon-btn" aria-label="Refresh" title="Refresh (F5)" onClick={refresh}><RefreshCw size={15} /></button>
          </div>
          {editingPath ? (
            <input
              className="path-input"
              aria-label="Location"
              defaultValue={path}
              autoFocus
              spellCheck={false}
              onFocus={e => e.currentTarget.select()}
              onBlur={() => setEditingPath(false)}
              onKeyDown={e => { if (e.key === "Enter") void navigateTyped(e.currentTarget.value); if (e.key === "Escape") setEditingPath(false); }}
            />
          ) : (
            <nav className="crumbs" aria-label="Location" onClick={e => { if (e.target === e.currentTarget) setEditingPath(true); }} title={`Click to type a location (${mod}+L)`}>
              {(info?.segments ?? [{ name: path, path }]).map((segment, i, all) => (
                <span key={segment.path} style={{ display: "contents" }}>
                  {i > 0 && <ChevronRight size={14} className="crumb-sep" aria-hidden="true" />}
                  <button type="button" className={`crumb ${i === all.length - 1 ? "current" : ""}`} aria-current={i === all.length - 1 ? "location" : undefined} onClick={() => navigate(segment.path)}>{segment.name}</button>
                </span>
              ))}
            </nav>
          )}
          <button type="button" className="search-trigger" onClick={() => setPaletteOpen(true)}><Search size={15} aria-hidden="true" /><span>Search files</span><kbd>{mod} K</kbd></button>
          <button type="button" className={`icon-btn ${trayOpen ? "on" : ""}`} aria-label="Transfers" title="Transfers" onClick={() => setTrayOpen(o => !o)} style={{ position: "relative" }}>
            <Activity size={17} />{activeTasks.length > 0 && <span className="dot" style={{ position: "absolute", top: 6, right: 6 }} />}
          </button>
        </header>

        {(view === "health" || view === "cleanup" || view === "storage" || view === "duplicates" || view === "devbloat") && (
          <div className="toolbar" style={{ paddingBottom: 0 }}>
            <div className="seg" role="tablist" aria-label="Storage and Cleanup">
              <button type="button" aria-pressed={view === "health"} onClick={() => setView("health")}>Health Overview</button>
              <button type="button" aria-pressed={view === "cleanup"} onClick={() => setView("cleanup")}>Deep Clean</button>
              <button type="button" aria-pressed={view === "storage"} onClick={() => setView("storage")}>Space Analyzer</button>
              <button type="button" aria-pressed={view === "duplicates"} onClick={() => setView("duplicates")}>Duplicates</button>
              <button type="button" aria-pressed={view === "devbloat"} onClick={() => setView("devbloat")}>Dev & Large Files</button>
            </div>
          </div>
        )}
        {(view === "monitor" || view === "startup" || view === "processes" || view === "apps" || view === "ostools" || view === "privacy") && (
          <div className="toolbar" style={{ paddingBottom: 0 }}>
            <div className="seg" role="tablist" aria-label="System and Boost">
              <button type="button" aria-pressed={view === "monitor"} onClick={() => setView("monitor")}>Live Monitor</button>
              <button type="button" aria-pressed={view === "startup"} onClick={() => setView("startup")}>Startup Apps</button>
              <button type="button" aria-pressed={view === "processes"} onClick={() => setView("processes")}>Processes</button>
              <button type="button" aria-pressed={view === "apps"} onClick={() => setView("apps")}>Installed Apps</button>
              <button type="button" aria-pressed={view === "ostools"} onClick={() => setView("ostools")}>Disk & Backup</button>
              <button type="button" aria-pressed={view === "privacy"} onClick={() => setView("privacy")}>Privacy & Shredder</button>
            </div>
          </div>
        )}
        {(view === "rules" || view === "customclean" || view === "audit") && (
          <div className="toolbar" style={{ paddingBottom: 0 }}>
            <div className="seg" role="tablist" aria-label="Automate">
              <button type="button" aria-pressed={view === "rules"} onClick={() => setView("rules")}>Organize Rules</button>
              <button type="button" aria-pressed={view === "customclean"} onClick={() => setView("customclean")}>Custom Cleaners</button>
              <button type="button" aria-pressed={view === "audit"} onClick={() => setView("audit")}>History Log</button>
            </div>
          </div>
        )}

        {view === "files" && fileView}
        {view === "search" && <SearchView currentPath={path} notify={notify} confirm={dialogs.confirm} onOpen={entry => void openEntry(entry)} onReveal={revealEntry} />}
        {view === "health" && <HealthView trashName={trashName} onOpenClean={() => setView("cleanup")} />}
        {view === "storage" && <StorageView path={path} trashName={trashName} onNavigate={navigate} onOpen={entry => void openEntry(entry)} onTrash={trash} onDuplicates={() => setView("duplicates")} />}
        {view === "duplicates" && <DuplicatesView path={path} trashName={trashName} onTrash={trash} onOpen={entry => void openEntry(entry)} />}
        {view === "cleanup" && <CleanupView trashName={trashName} onTrash={trash} />}
        {view === "devbloat" && <DevBloatView path={path} trashName={trashName} onTrash={trash} />}
        {view === "monitor" && <MonitorView />}
        {view === "startup" && <StartupView />}
        {view === "processes" && <ProcessView confirm={dialogs.confirm} />}
        {view === "apps" && <AppsView trashName={trashName} onTrash={trash} />}
        {view === "ostools" && <OsToolsBackupView currentPath={path} platform={platform} />}
        {view === "privacy" && <PrivacyShredView currentPath={path} confirm={dialogs.confirm} />}
        {view === "rules" && <RulesView currentPath={path} notify={notify} confirm={dialogs.confirm} onChanged={refresh} />}
        {view === "customclean" && <CustomCleanersView />}
        {view === "audit" && <AuditLogView />}
        {view === "settings" && <SettingsView settings={settings} update={update} platform={platform} shortcutError={shortcutError} />}

        <footer className="statusbar">
          <span className="truncate">{view === "files" ? (selectedEntries.length ? `${selectedEntries.length} selected · ${fmtBytes(selectedEntries.reduce((s, e) => s + (e.is_dir ? 0 : e.size), 0))}` : path) : " "}</span>
          {activeTasks.length > 0 ? (
            <button type="button" className="progress-inline" style={{ border: 0, background: "none", color: "inherit", padding: 0 }} onClick={() => setTrayOpen(true)}>
              <span>{activeTasks.length} transfer{activeTasks.length === 1 ? "" : "s"}</span>
              <span className="meter"><i style={{ width: `${percentOf(activeTasks[0].progress)}%` }} /></span>
            </button>
          ) : busy ? <span><LoaderCircle size={12} className="spin" /> Working…</span> : <span>{clipboard ? `${clipboard.entries.length} item${clipboard.entries.length === 1 ? "" : "s"} ${clipboard.cut ? "cut" : "copied"}` : ""}</span>}
        </footer>
      </main>

      {trayOpen && <Tray tasks={tasks} onPause={id => control(id, "pause")} onResume={id => control(id, "resume")} onCancel={id => control(id, "cancel")} onClear={() => setTasks({})} onClose={() => setTrayOpen(false)} />}
      {paletteOpen && (
        <Palette
          modLabel={mod}
          onClose={() => setPaletteOpen(false)}
          onPick={entry => { setPaletteOpen(false); void openEntry(entry); }}
          onReveal={entry => { setPaletteOpen(false); revealEntry(entry); }}
          onManage={() => { setPaletteOpen(false); setView("search"); }}
        />
      )}
      {pending && (
        <ConflictDialog
          conflicts={pending.conflicts}
          trashName={trashName}
          onClose={() => setPending(null)}
          onChoose={choice => { const job = pending; setPending(null); void startTransfer(job.sources, job.destination, job.move, choice); }}
        />
      )}
      {archive && (
        <ArchiveDialog
          {...archive}
          onClose={() => setArchive(null)}
          onExtract={() => {
            const job = archive;
            setArchive(null);
            setBusy(true);
            api.extractArchive(job.path, job.destination)
              .then(count => { notify("success", `Extracted ${count.toLocaleString()} items`); refresh(); })
              .catch(e => notify("error", errorText(e)))
              .finally(() => setBusy(false));
          }}
        />
      )}
      {comparison && <CompareDialog comparison={comparison} onClose={() => setComparison(null)} />}
      {dialogs.element}
      {notice && (
        <div className={`toast ${notice.kind}`} role="status" aria-live="polite">
          <span>{notice.text}</span>
          {notice.action && <button type="button" onClick={() => { notice.action?.run(); setNotice(null); }}>{notice.action.label}</button>}
        </div>
      )}
    </div>
  );
}

function SecondaryHead({ path, onNavigate }: { path: string; onNavigate: (path: string) => void }) {
  const [value, setValue] = useState(path);
  useEffect(() => setValue(path), [path]);
  return (
    <div className="pane-head">
      <button type="button" className="icon-btn" aria-label="Up one folder in second pane" onClick={() => {
        void api.pathInfo(path).then(info => { if (info.parent) onNavigate(info.parent); });
      }}><ArrowUp size={15} /></button>
      <input className="path" aria-label="Second pane location" value={value} spellCheck={false} onChange={e => setValue(e.target.value)} onKeyDown={e => { if (e.key === "Enter") onNavigate(value); if (e.key === "Escape") setValue(path); }} />
    </div>
  );
}
