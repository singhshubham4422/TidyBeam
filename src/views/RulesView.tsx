import { useEffect, useState } from "react";
import { ArrowRight, Check, LoaderCircle, Plus, ShieldCheck, Trash2, Workflow } from "lucide-react";
import { api, errorText, fmtBytes, type OrganizeRule, type RulePreview } from "../api";
import type { ConfirmOptions } from "../components/Dialogs";
import { FileIcon } from "../components/FileIcon";

type Props = {
  currentPath: string;
  notify: (kind: "success" | "error", text: string) => void;
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  onChanged: () => void;
};

const blank = (source: string): OrganizeRule => ({ id: "", name: "", source, destination: "", extensions: [], minAgeDays: 7, automatic: false });

export function RulesView({ currentPath, notify, confirm, onChanged }: Props) {
  const [rules, setRules] = useState<OrganizeRule[]>([]);
  const [draft, setDraft] = useState<OrganizeRule>(blank(currentPath));
  const [extensions, setExtensions] = useState("");
  const [preview, setPreview] = useState<RulePreview | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  useEffect(() => { api.listRules().then(setRules).catch(e => notify("error", errorText(e))); }, [notify]);

  const choose = (rule: OrganizeRule) => { setDraft(rule); setExtensions(rule.extensions.join(", ")); setPreview(null); setPicked(new Set()); };
  const startNew = () => { setDraft(blank(currentPath)); setExtensions(""); setPreview(null); setPicked(new Set()); };

  const save = async () => {
    const wasAutomatic = rules.some(r => r.id === draft.id && r.automatic);
    if (draft.automatic && !wasAutomatic) {
      const ok = await confirm({
        title: "Turn on automatic moves?",
        body: "TidyBeam will move matching files that are at least one day old from the source to the destination in the background, including files that already match. If a name is taken, both copies are kept.",
        confirmLabel: "Turn on",
      });
      if (!ok) return;
    }
    setBusy(true);
    try {
      const saved = await api.saveRule({ ...draft, extensions: extensions.split(/[\s,;]+/).map(v => v.trim().replace(/^\./, "")).filter(Boolean) });
      setRules(saved);
      const current = saved.find(r => r.id === draft.id) ?? saved.at(-1);
      if (current) choose(current);
      notify("success", current?.automatic ? "Rule saved and running automatically" : "Rule saved. Preview it to move files.");
    } catch (e) { notify("error", errorText(e)); }
    finally { setBusy(false); }
  };

  const runPreview = async () => {
    if (!draft.id) return;
    setBusy(true); setPreview(null); setPicked(new Set());
    try { setPreview(await api.previewRule(draft.id)); } catch (e) { notify("error", errorText(e)); }
    finally { setBusy(false); }
  };

  const remove = async () => {
    if (!draft.id || !(await confirm({ title: `Delete “${draft.name}”?`, body: "The rule is removed. Files it already moved stay where they are.", confirmLabel: "Delete rule", danger: true }))) return;
    setBusy(true);
    try { setRules(await api.deleteRule(draft.id)); startNew(); notify("success", "Rule deleted"); }
    catch (e) { notify("error", errorText(e)); }
    finally { setBusy(false); }
  };

  const apply = async () => {
    if (!draft.id || !picked.size) return;
    setBusy(true);
    try {
      const result = await api.applyRule(draft.id, [...picked]);
      notify("success", `Moved ${result.completed} file${result.completed === 1 ? "" : "s"}`);
      onChanged();
      await runPreview();
    } catch (e) { notify("error", errorText(e)); }
    finally { setBusy(false); }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div><h1>Rules</h1><p>Move files from one folder to another by type and age. Preview first, or let a rule run on its own.</p></div>
        <button type="button" className="btn" onClick={startNew}><Plus size={15} /> New rule</button>
      </header>
      <div className="columns">
        <section className="panel" style={{ padding: 16 }}>
          <div className="form">
            <label className="field"><span>Name</span><input className="input" value={draft.name} maxLength={80} placeholder="Sort old PDFs" onChange={e => setDraft(d => ({ ...d, name: e.target.value }))} /></label>
            <label className="field"><span>From folder</span>
              <span className="input-row"><input className="input" value={draft.source} spellCheck={false} placeholder="Full folder path" onChange={e => setDraft(d => ({ ...d, source: e.target.value }))} /><button type="button" className="btn" onClick={() => setDraft(d => ({ ...d, source: currentPath }))}>Use current</button></span>
            </label>
            <label className="field"><span>To folder</span>
              <span className="input-row"><input className="input" value={draft.destination} spellCheck={false} placeholder="Full folder path" onChange={e => setDraft(d => ({ ...d, destination: e.target.value }))} /><button type="button" className="btn" onClick={() => setDraft(d => ({ ...d, destination: currentPath }))}>Use current</button></span>
            </label>
            <div className="field-grid">
              <label className="field"><span>File types</span><input className="input" value={extensions} placeholder="pdf, docx (empty = all)" onChange={e => setExtensions(e.target.value)} /></label>
              <label className="field"><span>Older than (days)</span><input className="input" type="number" min={0} max={36500} value={draft.minAgeDays} onChange={e => setDraft(d => ({ ...d, minAgeDays: Math.max(0, Number(e.target.value) || 0) }))} /></label>
            </div>
            <label className="check-row" style={{ padding: 0, minHeight: 0 }}>
              <input type="checkbox" checked={draft.automatic} disabled={draft.minAgeDays < 1} onChange={e => setDraft(d => ({ ...d, automatic: e.target.checked }))} />
              <span><strong style={{ fontWeight: 500 }}>Run automatically</strong><span className="sub muted small" style={{ display: "block" }}>Needs files to be at least 1 day old. Checks on changes and every 15 minutes while TidyBeam is open.</span></span>
            </label>
            <div className="notice"><ShieldCheck size={16} /><span>Manual rules never move anything until you pick files and confirm. If a name is taken, both copies are kept.</span></div>
            <div className="actions">
              <button type="button" className="btn btn-primary" disabled={busy || !draft.name.trim() || !draft.source.trim() || !draft.destination.trim()} onClick={() => void save()}>{busy ? <LoaderCircle size={15} className="spin" /> : <Check size={15} />} Save rule</button>
              {draft.id && <button type="button" className="btn" disabled={busy} onClick={() => void runPreview()}>Preview</button>}
              {draft.id && <button type="button" className="icon-btn" aria-label="Delete rule" disabled={busy} onClick={() => void remove()}><Trash2 size={16} /></button>}
            </div>
          </div>
          <div style={{ marginTop: 20 }}>
            <div className="nav-label" style={{ padding: "0 0 6px" }}>Saved rules · {rules.length}</div>
            {rules.length === 0 && <div className="muted small">No rules yet.</div>}
            {rules.map(rule => (
              <button key={rule.id} type="button" className={`panel-row clickable ${draft.id === rule.id ? "" : ""}`} style={draft.id === rule.id ? { background: "var(--accent-soft)" } : undefined} onClick={() => choose(rule)}>
                <Workflow size={16} aria-hidden="true" />
                <span className="grow"><span className="truncate" style={{ display: "block" }}>{rule.name}</span><span className="sub truncate">{rule.extensions.length ? rule.extensions.map(e => `.${e}`).join(", ") : "All files"} · older than {rule.minAgeDays} days</span></span>
                <span className={`pill ${rule.automatic ? "live" : ""}`}>{rule.automatic ? "Auto" : "Manual"}</span>
              </button>
            ))}
          </div>
        </section>
        <section className="panel">
          <h2>
            <span>Matching files</span>
            {preview && preview.files.length > 0 && <button type="button" className="btn btn-sm" onClick={() => setPicked(picked.size === preview.files.length ? new Set() : new Set(preview.files.map(f => f.path)))}>{picked.size === preview.files.length ? "Select none" : "Select all"}</button>}
          </h2>
          {!preview ? (
            <div className="state"><span className="badge"><Workflow size={20} /></span><p>{draft.id ? "Preview the rule to see which files it would move." : "Save a rule, then preview it."}</p></div>
          ) : preview.files.length === 0 ? (
            <div className="state"><span className="badge"><Check size={20} /></span><h3>Nothing to move</h3><p>No files match right now.</p></div>
          ) : (
            <>
              {preview.files.map(file => (
                <label key={file.path} className="check-row" title={file.path}>
                  <input type="checkbox" checked={picked.has(file.path)} onChange={e => setPicked(old => { const next = new Set(old); if (e.target.checked) next.add(file.path); else next.delete(file.path); return next; })} />
                  <FileIcon entry={file} />
                  <span style={{ flex: 1, minWidth: 0 }} className="truncate">{file.name}</span>
                  <span className="num muted small">{fmtBytes(file.size)}</span>
                </label>
              ))}
              <div className="actions" style={{ padding: "12px", justifyContent: "space-between" }}>
                <span className="muted small truncate">To {preview.rule.destination}</span>
                <button type="button" className="btn btn-primary" disabled={busy || !picked.size} onClick={() => void apply()}><ArrowRight size={15} /> Move {picked.size || ""}</button>
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
}
