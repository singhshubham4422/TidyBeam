import { File, Folder } from "lucide-react";
import { fmtBytes, type ArchiveEntry, type FolderComparison, type TransferConflict } from "../api";
import { Modal } from "./Dialogs";

export function ConflictDialog({ conflicts, trashName, onChoose, onClose }: {
  conflicts: TransferConflict[]; trashName: string; onChoose: (choice: "skip" | "keep-both" | "replace") => void; onClose: () => void;
}) {
  const many = conflicts.length > 1;
  return (
    <Modal
      title={many ? `${conflicts.length} items already exist here` : `“${conflicts[0].name}” already exists here`}
      onClose={onClose}
      wide
      actions={<>
        <button type="button" className="btn" onClick={() => onChoose("skip")}>Skip</button>
        <button type="button" className="btn" onClick={() => onChoose("keep-both")}>Keep both</button>
        <button type="button" className="btn btn-dark" onClick={() => onChoose("replace")}>Replace</button>
      </>}
    >
      <p>Your choice applies to every conflict. Replaced items go to the {trashName}, so you can restore them.</p>
      <div className="modal-list">
        {conflicts.slice(0, 50).map(conflict => (
          <div className="item" key={conflict.destination}>
            {conflict.is_dir ? <Folder size={16} className="icon-folder" aria-hidden="true" /> : <File size={16} className="icon-file" aria-hidden="true" />}
            <span className="truncate" title={conflict.destination}>{conflict.name}</span>
          </div>
        ))}
        {conflicts.length > 50 && <div className="item muted">and {conflicts.length - 50} more</div>}
      </div>
    </Modal>
  );
}

export function ArchiveDialog({ path, entries, loading, destination, onExtract, onClose }: {
  path: string; entries: ArchiveEntry[]; loading: boolean; destination: string; onExtract: () => void; onClose: () => void;
}) {
  const total = entries.reduce((sum, e) => sum + e.size, 0);
  return (
    <Modal
      title={path.split(/[\\/]/).at(-1) || "Archive"}
      onClose={onClose}
      wide
      actions={<>
        <span className="muted small truncate" style={{ marginRight: "auto", alignSelf: "center" }}>Extracts to {destination}</span>
        <button type="button" className="btn" onClick={onClose}>Close</button>
        <button type="button" className="btn btn-primary" disabled={loading || !entries.length} onClick={onExtract}>Extract here</button>
      </>}
    >
      <p>{loading ? "Reading archive…" : `${entries.length.toLocaleString()} entries · ${fmtBytes(total)} uncompressed`}</p>
      <div className="modal-list">
        {entries.slice(0, 500).map((entry, i) => (
          <div className="item" key={`${entry.path}-${i}`}>
            {entry.is_dir ? <Folder size={16} className="icon-folder" aria-hidden="true" /> : <File size={16} className="icon-file" aria-hidden="true" />}
            <span className="truncate" style={{ flex: 1 }}>{entry.path}</span>
            <span className="muted">{entry.is_dir ? "" : fmtBytes(entry.size)}</span>
          </div>
        ))}
        {entries.length > 500 && <div className="item muted">and {entries.length - 500} more</div>}
      </div>
    </Modal>
  );
}

export function CompareDialog({ comparison, onClose }: { comparison: FolderComparison; onClose: () => void }) {
  const label = { "left-only": "Only left", "right-only": "Only right", different: "Different" } as const;
  return (
    <Modal title="Compare folders" onClose={onClose} wide actions={<button type="button" className="btn btn-primary" onClick={onClose}>Done</button>}>
      <p>{comparison.leftOnly} only on the left · {comparison.rightOnly} only on the right · {comparison.different} different</p>
      <div className="modal-list">
        {comparison.items.length === 0 && <div className="item">These folders match by name, type, size and modified time.</div>}
        {comparison.items.map(item => (
          <div className="item" key={`${item.status}:${item.name}`}>
            <span className={`pill ${item.status === "different" ? "" : "live"}`} style={{ minWidth: 84, textAlign: "center" }}>{label[item.status]}</span>
            <span className="truncate" style={{ flex: 1 }}>{item.name}</span>
            <span className="muted">{item.left ? fmtBytes(item.left.size) : "—"} / {item.right ? fmtBytes(item.right.size) : "—"}</span>
          </div>
        ))}
      </div>
    </Modal>
  );
}
