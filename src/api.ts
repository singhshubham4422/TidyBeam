import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export type FileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  modified: number;
  extension: string;
  hidden: boolean;
  executable: boolean;
};
export type SortKey = "name" | "size" | "modified" | "kind";
export type ListResult = { entries: FileEntry[]; total: number };
export type PathInfo = { path: string; parent: string | null; segments: { name: string; path: string }[] };
export type RuntimeCapabilities = {
  os: "windows" | "macos" | "linux";
  arch: string;
  isElevated: boolean;
  isPackagedStore: boolean;
  capabilities: {
    registryClean: boolean;
    packageManagerClean: boolean;
    smartDiskHealth: boolean;
    tempSensors: boolean;
    nativeScheduler: boolean;
    emptyWorkingSet: boolean;
    freeSpaceWipe: boolean;
  };
};
export type PlatformInfo = {
  os: "windows" | "macos" | "linux"; trashName: string; separator: string; home: string; version: string;
  runtime?: RuntimeCapabilities;
};
export type Place = { name: string; path: string; kind: string };
export type Drive = { name: string; path: string; total: number; available: number; file_system: string; removable: boolean };
export type TransferConflict = { source: string; destination: string; name: string; is_dir: boolean };
export type TransferSummary = { completed: number; skipped: number; skipped_links: number };
export type TransferStatus = "queued" | "running" | "paused" | "completed" | "failed" | "cancelled";
export type TransferProgress = {
  taskId: string; status: TransferStatus; completedBytes: number; totalBytes: number;
  completedItems: number; totalItems: number; currentName: string; speedBytesPerSecond: number; message?: string | null;
};
export type TrashResult = { trashed: number; failed: { path: string; error: string }[] };
export type CategoryBreakdown = {
  media: number;
  archives: number;
  code: number;
  documents: number;
  appData: number;
  other: number;
};
export type FolderSize = { name: string; path: string; bytes: number; allocated_bytes: number; files: number; is_dir: boolean };
export type Analysis = {
  path: string; total_bytes: number; allocated_bytes: number; file_count: number; folder_count: number;
  scanned_at: number; categories: CategoryBreakdown; largest: FileEntry[]; folders: FolderSize[];
};
export type AnalysisProgress = {
  scanId: string; filesScanned: number; foldersScanned: number; bytesScanned: number; topFolders: FolderSize[];
};
export type DuplicateGroup = { hash: string; size: number; files: FileEntry[] };
export type CleanupCandidate = { name: string; path: string; category: string; bytes: number; item_count: number; modified: number };
export type SafetyBadge = "safe" | "sessionReset" | "appOpen" | "permanent";
export type CleanerFileItem = { name: string; path: string; bytes: number; modified: number };
export type CleanerRuleSummary = {
  id: string; name: string; category: string; description: string; badge: SafetyBadge;
  appRunning: boolean; runningProcess?: string | null; totalBytes: number; itemCount: number;
  sqliteTargets: string[]; cookieDbs: string[];
};
export type SystemSnapshot = {
  osName: string; kernelVersion: string; hostName: string; cpuBrand: string; cpuUsage: number; coreCount: number;
  cores: { name: string; usage: number }[]; memTotal: number; memUsed: number; memAvailable: number;
  swapTotal: number; swapUsed: number; uptimeSecs: number;
  networks: { name: string; receivedBytes: number; transmittedBytes: number }[];
  temperatures: { label: string; celsius: number }[];
};
export type ProcessEntry = {
  pid: number; parentPid?: number | null; name: string; cpuUsage: number; memoryBytes: number;
  status: string; exePath: string; protected: boolean;
};
export type StartupItem = { id: string; name: string; command: string; scope: string; enabled: boolean; path: string };
export type HealthPillar = {
  id: string; title: string; status: "good" | "review" | "attention"; summary: string;
  reclaimableBytes: number; fixable: boolean;
};
export type HealthReport = { score: number; totalReclaimable: number; pillars: HealthPillar[] };
export type DevBloatItem = { name: string; path: string; kind: string; bytes: number; modified: number };
export type ImportedCleanerRule = { name: string; section: string; sourceFormat: string; osCompat: string[]; targets: string[] };
export type AuditEntry = { id: number; timestamp: number; action: string; summary: string; bytesReclaimed: number; itemsCount: number };
export type InstalledApp = { name: string; version: string; publisher: string; location: string; source: string };
export type LeftoverFolder = { name: string; path: string; bytes: number };
export type DiskHealthInfo = {
  name: string; mountPoint: string; fileSystem: string; kind: string;
  totalBytes: number; availableBytes: number; usedPct: number; removable: boolean; status: string;
};
export type ArchiveEntry = { name: string; path: string; is_dir: boolean; size: number; compressed_size: number };
export type IndexStatus = { root: string; indexed: number; watching: boolean; watch_error?: string | null };
export type IndexRoot = { root: string; watch: boolean; watching: boolean; indexed: number; indexedAt: number };
export type OrganizeRule = { id: string; name: string; source: string; destination: string; extensions: string[]; minAgeDays: number; automatic: boolean };
export type RulePreview = { rule: OrganizeRule; files: FileEntry[] };
export type RuleRunActivity = { ruleId: string; ruleName: string; completed: number };
export type FolderComparison = {
  left: string; right: string; leftOnly: number; rightOnly: number; different: number;
  items: { name: string; status: "left-only" | "right-only" | "different"; left: FileEntry | null; right: FileEntry | null }[];
};

export const api = {
  platformInfo: () => invoke<PlatformInfo>("platform_info"),
  userPlaces: () => invoke<Place[]>("user_places"),
  listDrives: () => invoke<Drive[]>("list_drives"),
  pathInfo: (path: string) => invoke<PathInfo>("path_info", { path }),
  listEntries: (args: { path: string; offset: number; limit: number; showHidden: boolean; sort: SortKey; descending: boolean; filter: string; force: boolean }) =>
    invoke<ListResult>("list_entries", args),
  listEntriesRange: (args: { path: string; offset: number; limit: number; showHidden: boolean; sort: SortKey; descending: boolean; filter: string }) =>
    invoke<ListResult>("list_entries_range", args),
  thumbnailPath: (path: string) => invoke<string>("thumbnail_path", { path }),
  compareFolders: (left: string, right: string, showHidden: boolean) => invoke<FolderComparison>("compare_folders", { left, right, showHidden }),
  readTextPreview: (path: string) => invoke<{ text: string; truncated: boolean }>("read_text_preview", { path }),
  openItem: (path: string, confirmed = false) => invoke<void>("open_item", { path, confirmed }),
  revealItem: (path: string) => invoke<void>("reveal_item", { path }),
  createFolder: (path: string, name: string) => invoke<string>("create_folder", { path, name }),
  renamePath: (path: string, newName: string) => invoke<string>("rename_path", { path, newName }),
  undoCreatedFolder: (path: string) => invoke<void>("undo_created_folder", { path }),
  trashPaths: (paths: string[]) => invoke<TrashResult>("trash_paths", { paths }),
  deletePermanently: (paths: string[]) => invoke<number>("delete_permanently", { paths }),
  previewTransfer: (sources: string[], destination: string) => invoke<{ conflicts: TransferConflict[] }>("preview_transfer", { sources, destination }),
  startTransfer: (taskId: string, sources: string[], destination: string, moveItems: boolean, conflict: "skip" | "keep-both" | "replace") =>
    invoke<void>("start_transfer", { taskId, sources, destination, moveItems, conflict }),
  pauseTransfer: (taskId: string) => invoke<void>("pause_transfer", { taskId }),
  resumeTransfer: (taskId: string) => invoke<void>("resume_transfer", { taskId }),
  cancelTransfer: (taskId: string) => invoke<void>("cancel_transfer", { taskId }),
  listRules: () => invoke<OrganizeRule[]>("list_rules"),
  saveRule: (rule: OrganizeRule) => invoke<OrganizeRule[]>("save_rule", { rule }),
  previewRule: (id: string) => invoke<RulePreview>("preview_rule", { id }),
  applyRule: (id: string, selected: string[]) => invoke<TransferSummary>("apply_rule", { id, selected }),
  deleteRule: (id: string) => invoke<OrganizeRule[]>("delete_rule", { id }),
  analyzeFolder: (path: string, scanId: string, force = false) => invoke<Analysis>("analyze_folder", { path, scanId, force }),
  getCachedAnalysis: (path: string) => invoke<Analysis | null>("get_cached_analysis", { path }),
  cancelAnalysis: (scanId: string) => invoke<void>("cancel_analysis", { scanId }),
  findDuplicates: (path: string) => invoke<DuplicateGroup[]>("find_duplicates", { path }),
  scanCleanup: () => invoke<CleanupCandidate[]>("scan_cleanup"),
  scanDeepCleanup: (force = false) => invoke<CleanerRuleSummary[]>("scan_deep_cleanup", { force }),
  getCleanerItems: (ruleId: string, offset: number, limit: number) => invoke<CleanerFileItem[]>("get_cleaner_items", { ruleId, offset, limit }),
  cleanRules: (ruleIds: string[], permanent = false) => invoke<TrashResult>("clean_rules", { ruleIds, permanent }),
  vacuumSqliteTargets: (paths: string[]) => invoke<number>("vacuum_sqlite_targets", { paths }),
  cleanCookiesWhitelist: (paths: string[], whitelist: string[]) => invoke<number>("clean_cookies_whitelist", { paths, whitelist }),
  emptyOsTrash: () => invoke<void>("empty_os_trash"),
  systemSnapshot: () => invoke<SystemSnapshot>("system_snapshot"),
  listProcesses: () => invoke<ProcessEntry[]>("list_processes"),
  terminateProcess: (pid: number) => invoke<void>("terminate_process", { pid }),
  listStartupItems: () => invoke<StartupItem[]>("list_startup_items"),
  toggleStartupItem: (id: string, enable: boolean) => invoke<void>("toggle_startup_item", { id, enable }),
  runHealthCheck: () => invoke<HealthReport>("run_health_check"),
  scanDevBloat: (path: string, inactiveDays: number) => invoke<DevBloatItem[]>("scan_dev_bloat", { path, inactiveDays }),
  shredPaths: (paths: string[], passes: number) => invoke<number>("shred_paths", { paths, passes }),
  wipeFreeSpace: (path: string, maxMb: number) => invoke<number>("wipe_free_space", { path, maxMb }),
  importCustomCleaners: (path: string) => invoke<ImportedCleanerRule[]>("import_custom_cleaners", { path }),
  listAuditLog: (limit?: number) => invoke<AuditEntry[]>("list_audit_log", { limit }),
  listInstalledApps: () => invoke<InstalledApp[]>("list_installed_apps"),
  findAppLeftovers: (appName: string) => invoke<LeftoverFolder[]>("find_app_leftovers", { appName }),
  listDiskHealth: () => invoke<DiskHealthInfo[]>("list_disk_health"),
  openOsSettings: (target: string) => invoke<void>("open_os_settings", { target }),
  backupFolder: (source: string, destinationDir: string, archiveName: string) =>
    invoke<string>("backup_folder", { source, destinationDir, archiveName }),
  listArchive: (path: string) => invoke<ArchiveEntry[]>("list_archive", { path }),
  extractArchive: (path: string, destination: string) => invoke<number>("extract_archive", { path, destination }),
  createZip: (paths: string[], destination: string, archiveName: string) => invoke<string>("create_zip", { paths, destination, archiveName }),
  indexFolder: (path: string) => invoke<IndexStatus>("index_folder", { path }),
  indexStatus: (path: string) => invoke<IndexStatus>("index_status", { path }),
  listIndexRoots: () => invoke<IndexRoot[]>("list_index_roots"),
  removeIndexRoot: (root: string) => invoke<void>("remove_index_root", { root }),
  setIndexWatch: (root: string, watch: boolean) => invoke<void>("set_index_watch", { root, watch }),
  searchIndex: (path: string | null, query: string, limit: number) => invoke<FileEntry[]>("search_index", { path, query, limit }),
};

export const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

export const fmtBytes = (n: number) => {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let value = n / 1024;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) { value /= 1024; i++; }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[i]}`;
};

const dateFormat = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
export const fmtDate = (seconds: number) => {
  if (!seconds) return "—";
  const date = new Date(seconds * 1000);
  const today = new Date();
  if (date.toDateString() === today.toDateString()) return `Today, ${timeFormat.format(date)}`;
  return dateFormat.format(date);
};

export const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif", "ico"]);
export const VIDEO_EXTENSIONS = new Set(["mp4", "webm", "mov", "m4v", "ogv"]);
export const AUDIO_EXTENSIONS = new Set(["mp3", "wav", "ogg", "oga", "m4a", "flac", "aac", "opus"]);
export const TEXT_EXTENSIONS = new Set([
  "txt", "md", "markdown", "log", "csv", "tsv", "json", "jsonc", "yaml", "yml", "toml", "ini", "cfg", "conf", "xml", "html", "htm", "css", "scss",
  "js", "jsx", "ts", "tsx", "mjs", "cjs", "rs", "go", "py", "rb", "java", "kt", "c", "h", "cpp", "hpp", "cs", "swift", "sh", "bash", "zsh", "ps1",
  "sql", "env", "gitignore", "dockerfile", "makefile", "lock", "svg",
]);
export const ARCHIVE_EXTENSIONS = new Set(["zip"]);

export const assetUrl = (path: string) => convertFileSrc(path);

/** Separator-aware, OS-case-normalized containment check (prevents sibling prefix matches). */
export const sameOrDescendant = (path: string, parent: string, os: PlatformInfo["os"] = "windows") => {
  const clean = (p: string) => (p.length > 1 ? p.replace(/[\\/]+$/, "") : p);
  const norm = (p: string) => (os === "linux" ? clean(p) : clean(p).toLowerCase());
  const a = norm(path);
  const b = norm(parent);
  if (!a || !b) return false;
  if (a === b) return true;
  if (b.endsWith("/") || b.endsWith("\\")) return a.startsWith(b);
  return a.startsWith(`${b}/`) || a.startsWith(`${b}\\`);
};

export const kindLabel = (entry: FileEntry) => {
  if (entry.is_dir) return "Folder";
  const ext = entry.extension;
  if (!ext) return "File";
  if (IMAGE_EXTENSIONS.has(ext)) return `${ext.toUpperCase()} image`;
  if (VIDEO_EXTENSIONS.has(ext)) return `${ext.toUpperCase()} video`;
  if (AUDIO_EXTENSIONS.has(ext)) return `${ext.toUpperCase()} audio`;
  if (ext === "pdf") return "PDF document";
  if (ext === "zip") return "ZIP archive";
  return `${ext.toUpperCase()} file`;
};
