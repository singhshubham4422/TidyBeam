import { useEffect, useState } from "react";
import { Files } from "lucide-react";
import { api, assetUrl, AUDIO_EXTENSIONS, fmtBytes, fmtDate, IMAGE_EXTENSIONS, kindLabel, TEXT_EXTENSIONS, VIDEO_EXTENSIONS, type FileEntry } from "../api";
import { FileIcon } from "./FileIcon";

type Props = {
  entries: FileEntry[];
  folderName: string;
  revealLabel: string;
  onOpen: (entry: FileEntry) => void;
  onReveal: (entry: FileEntry) => void;
};

const PREVIEW_IMAGE_MAX = 60 * 1024 * 1024;

function Preview({ entry }: { entry: FileEntry }) {
  const [text, setText] = useState<{ path: string; value: string; truncated: boolean } | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const ext = entry.extension;
  const wantsText = !entry.is_dir && TEXT_EXTENSIONS.has(ext) && ext !== "svg";

  useEffect(() => {
    setFailed(null);
    if (!wantsText) return;
    let current = true;
    // Small delay so quickly moving through a list doesn't read every file.
    const timer = window.setTimeout(() => {
      api.readTextPreview(entry.path)
        .then(result => { if (current) setText({ path: entry.path, value: result.text, truncated: result.truncated }); })
        .catch(() => { if (current) setText(null); });
    }, 120);
    return () => { current = false; window.clearTimeout(timer); };
  }, [entry.path, wantsText]);

  if (entry.is_dir) return <div className="preview-box"><FileIcon entry={entry} size={44} /></div>;
  if (failed !== entry.path && IMAGE_EXTENSIONS.has(ext) && entry.size < PREVIEW_IMAGE_MAX) {
    return <div className="preview-box"><img src={assetUrl(entry.path)} alt="" decoding="async" onError={() => setFailed(entry.path)} /></div>;
  }
  if (VIDEO_EXTENSIONS.has(ext)) return <div className="preview-box"><video src={assetUrl(entry.path)} controls preload="metadata" /></div>;
  if (AUDIO_EXTENSIONS.has(ext)) return <div className="preview-box"><audio src={assetUrl(entry.path)} controls preload="metadata" /></div>;
  if (wantsText && text?.path === entry.path) {
    return <pre className="preview-text">{text.value}{text.truncated ? "\n…" : ""}</pre>;
  }
  return <div className="preview-box"><FileIcon entry={entry} size={44} /></div>;
}

export function Details({ entries, folderName, revealLabel, onOpen, onReveal }: Props) {
  if (entries.length === 0) {
    return (
      <aside className="details" aria-label="Details">
        <div className="preview-box"><Files size={40} strokeWidth={1.5} aria-hidden="true" /></div>
        <div><h2>{folderName}</h2><div className="muted small">Select an item to see its details.</div></div>
      </aside>
    );
  }
  if (entries.length > 1) {
    const bytes = entries.reduce((sum, e) => sum + (e.is_dir ? 0 : e.size), 0);
    const folders = entries.filter(e => e.is_dir).length;
    return (
      <aside className="details" aria-label="Details">
        <div className="preview-box"><Files size={40} strokeWidth={1.5} aria-hidden="true" /></div>
        <div><h2>{entries.length} items selected</h2><div className="muted small">{folders ? `${folders} folder${folders === 1 ? "" : "s"} · ` : ""}{fmtBytes(bytes)} in files</div></div>
      </aside>
    );
  }
  const entry = entries[0];
  return (
    <aside className="details" aria-label="Details">
      <Preview entry={entry} />
      <div><h2>{entry.name}</h2><div className="muted small">{entry.is_dir ? "Folder" : fmtBytes(entry.size)}</div></div>
      <dl className="meta">
        <dt>Kind</dt><dd>{kindLabel(entry)}{entry.is_symlink ? " (link)" : ""}</dd>
        <dt>Modified</dt><dd>{fmtDate(entry.modified)}</dd>
        <dt>Location</dt><dd>{entry.path}</dd>
      </dl>
      <div className="actions">
        <button type="button" className="btn btn-dark" onClick={() => onOpen(entry)}>Open</button>
        <button type="button" className="btn" onClick={() => onReveal(entry)}>{revealLabel}</button>
      </div>
    </aside>
  );
}
