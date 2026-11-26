import { Activity, BarChart3, Download, FileText, Folder, HardDrive, Home, Image, Monitor, Music, Search, Settings, Sparkles, Star, Usb, Workflow, X } from "lucide-react";
import { fmtBytes, sameOrDescendant, type Drive, type Place } from "../api";

export type ToolView = "files" | "search" | "storage" | "duplicates" | "cleanup" | "devbloat" | "health" | "monitor" | "startup" | "processes" | "apps" | "ostools" | "privacy" | "rules" | "customclean" | "audit" | "settings";

const PLACE_ICONS: Record<string, typeof Home> = { home: Home, desktop: Monitor, documents: FileText, downloads: Download, pictures: Image, music: Music };

type Props = {
  view: ToolView;
  currentPath: string;
  places: Place[];
  favorites: string[];
  drives: Drive[];
  onNavigate: (path: string) => void;
  onView: (view: ToolView) => void;
  onRemoveFavorite: (path: string) => void;
  samePath: (a: string, b: string) => boolean;
};

const lastName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) || path;

export function Sidebar({ view, currentPath, places, favorites, drives, onNavigate, onView, onRemoveFavorite, samePath }: Props) {
  const activeDrive = drives
    .filter(d => sameOrDescendant(currentPath, d.path))
    .sort((a, b) => b.path.length - a.path.length)[0]?.path;
  const placeItem = (name: string, path: string, Icon: typeof Home, removable = false) => {
    const active = view === "files" && samePath(path, currentPath);
    return (
      <button key={path} type="button" className={`nav-item ${active ? "active" : ""}`} aria-current={active ? "page" : undefined} title={path} onClick={() => onNavigate(path)}>
        <Icon size={18} strokeWidth={1.75} aria-hidden="true" />
        <span className="label-text truncate">{name}</span>
        {removable && (
          <span
            className="remove label-text"
            role="button"
            tabIndex={-1}
            aria-label={`Remove ${name} from favorites`}
            onClick={event => { event.stopPropagation(); onRemoveFavorite(path); }}
          ><X size={13} /></span>
        )}
      </button>
    );
  };
  const tool = (id: ToolView, label: string, Icon: typeof Home) => (
    <button key={id} type="button" className={`nav-item ${view === id ? "active" : ""}`} aria-current={view === id ? "page" : undefined} title={label} onClick={() => onView(id)}>
      <Icon size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text">{label}</span>
    </button>
  );
  return (
    <aside className="sidebar" aria-label="Sidebar">
      <div className="brand"><span className="brand-mark"><Folder size={16} strokeWidth={2} aria-hidden="true" /></span><span className="label-text">TidyBeam</span></div>

      <nav className="nav-group" aria-label="Places">
        {places.map(place => placeItem(place.name, place.path, PLACE_ICONS[place.kind] ?? Folder))}
      </nav>

      <div className="nav-group">
        <div className="nav-label">Favorites</div>
        {favorites.length
          ? favorites.map(path => placeItem(lastName(path), path, Star, true))
          : <div className="nav-label" style={{ fontWeight: 400 }}>Pin folders with the star button.</div>}
      </div>

      {drives.length > 0 && (
        <div className="nav-group">
          <div className="nav-label">Drives</div>
          {drives.map(drive => {
            const used = Math.max(0, drive.total - drive.available);
            const pct = drive.total ? Math.round((used / drive.total) * 100) : 0;
            const Icon = drive.removable ? Usb : HardDrive;
            const active = view === "files" && activeDrive === drive.path;
            return (
              <button key={drive.path} type="button" className={`nav-item drive ${active ? "active" : ""}`} title={drive.path} onClick={() => onNavigate(drive.path)} style={{ alignItems: "stretch" }}>
                <span className="drive-top"><span style={{ display: "flex", gap: 10, alignItems: "center", minWidth: 0 }}><Icon size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text truncate">{drive.name}</span></span><span className="muted small drive-detail">{pct}%</span></span>
                <span className={`meter drive-detail ${pct >= 90 ? "warn" : ""}`} aria-hidden="true"><i style={{ width: `${pct}%` }} /></span>
                <span className="muted small drive-detail">{fmtBytes(used)} of {fmtBytes(drive.total)}</span>
              </button>
            );
          })}
        </div>
      )}

      <div className="sidebar-spacer" />
      <nav className="nav-group" aria-label="Workspaces">
        <div className="nav-label">Workspaces</div>
        <button type="button" className={`nav-item ${view === "files" || view === "search" ? "active" : ""}`} onClick={() => onView("files")}>
          <Folder size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text">Files & Search</span>
        </button>
        <button type="button" className={`nav-item ${view === "health" || view === "cleanup" || view === "storage" || view === "duplicates" || view === "devbloat" ? "active" : ""}`} onClick={() => onView("health")}>
          <Sparkles size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text">Storage & Clean</span>
        </button>
        <button type="button" className={`nav-item ${view === "monitor" || view === "startup" || view === "processes" || view === "apps" || view === "ostools" || view === "privacy" ? "active" : ""}`} onClick={() => onView("monitor")}>
          <Activity size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text">System & Boost</span>
        </button>
        <button type="button" className={`nav-item ${view === "rules" || view === "customclean" || view === "audit" ? "active" : ""}`} onClick={() => onView("rules")}>
          <Workflow size={18} strokeWidth={1.75} aria-hidden="true" /><span className="label-text">Automate</span>
        </button>
        {tool("search", "Search Index", Search)}
        {tool("storage", "Space Analyzer", BarChart3)}
        {tool("settings", "Settings", Settings)}
      </nav>
    </aside>
  );
}
