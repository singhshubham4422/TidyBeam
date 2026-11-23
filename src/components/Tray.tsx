import { Pause, Play, X } from "lucide-react";
import { fmtBytes, type TransferProgress } from "../api";

export type TransferTask = { move: boolean; destination: string; count: number; progress: TransferProgress };
export const isActive = (status: TransferProgress["status"]) => status === "queued" || status === "running" || status === "paused";
export const percentOf = (p: TransferProgress) =>
  p.totalBytes ? Math.min(100, (p.completedBytes / p.totalBytes) * 100) : p.totalItems ? Math.min(100, (p.completedItems / p.totalItems) * 100) : p.status === "completed" ? 100 : 0;

type Props = {
  tasks: Record<string, TransferTask>;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onClear: () => void;
  onClose: () => void;
};

const destinationName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) || path;

export function Tray({ tasks, onPause, onResume, onCancel, onClear, onClose }: Props) {
  const list = Object.entries(tasks).reverse();
  const anyActive = list.some(([, t]) => isActive(t.progress.status));
  return (
    <section className="tray" aria-label="Transfers">
      <div className="tray-head">
        <strong>Transfers</strong>
        <div className="actions">
          {list.length > 0 && <button type="button" className="btn btn-sm btn-ghost" disabled={anyActive} onClick={onClear}>Clear</button>}
          <button type="button" className="icon-btn" aria-label="Close transfers" onClick={onClose}><X size={16} /></button>
        </div>
      </div>
      <div className="tray-list">
        {list.length === 0 && <div className="state"><p>Copies and moves show up here.</p></div>}
        {list.map(([id, task]) => {
          const p = task.progress;
          const pct = percentOf(p);
          const verb = task.move ? "Moving" : "Copying";
          const title = isActive(p.status)
            ? `${verb} ${task.count} item${task.count === 1 ? "" : "s"} to ${destinationName(task.destination)}`
            : `${task.move ? "Moved" : "Copied"} to ${destinationName(task.destination)}`;
          const detail = p.status === "running" ? `${p.currentName} · ${fmtBytes(p.speedBytesPerSecond)}/s`
            : p.status === "paused" ? "Paused"
            : p.status === "failed" ? p.message || "Failed"
            : p.status === "cancelled" ? "Cancelled"
            : p.status === "completed" ? p.message || "Done"
            : "Waiting…";
          return (
            <article key={id} className={`task ${p.status === "failed" ? "failed" : ""}`}>
              <div className="task-top">
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontWeight: 500 }}>{p.status === "failed" ? `${verb} failed` : title}</div>
                  <div className="muted small" style={{ wordBreak: "break-word" }}>{detail}</div>
                </div>
                {isActive(p.status) && (
                  <div className="actions">
                    {p.status === "paused"
                      ? <button type="button" className="icon-btn" aria-label="Resume" onClick={() => onResume(id)}><Play size={15} /></button>
                      : <button type="button" className="icon-btn" aria-label="Pause" onClick={() => onPause(id)}><Pause size={15} /></button>}
                    <button type="button" className="icon-btn" aria-label="Cancel" onClick={() => onCancel(id)}><X size={15} /></button>
                  </div>
                )}
              </div>
              <div className="progress" role="progressbar" aria-label={title} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)}><i style={{ width: `${pct}%` }} /></div>
              <div className="muted small" style={{ display: "flex", justifyContent: "space-between" }}>
                <span>{p.totalBytes ? `${fmtBytes(p.completedBytes)} of ${fmtBytes(p.totalBytes)}` : `${p.completedItems} of ${p.totalItems} items`}</span>
                <span>{Math.round(pct)}%</span>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
