import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";

export function Modal({ title, children, actions, onClose, wide = false }: { title: string; children?: ReactNode; actions?: ReactNode; onClose: () => void; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first = ref.current?.querySelector<HTMLElement>("[data-autofocus], input, button.btn-primary, button.btn-danger, button");
    first?.focus();
    return () => previous?.focus?.();
  }, []);
  return (
    <div className="backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <div
        ref={ref}
        className={`modal ${wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}
      >
        <div className="modal-head">
          <h2>{title}</h2>
          <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}><X size={16} /></button>
        </div>
        {children && <div className="modal-body">{children}</div>}
        {actions && <div className="modal-actions">{actions}</div>}
      </div>
    </div>
  );
}

export type ConfirmOptions = { title: string; body?: ReactNode; confirmLabel?: string; cancelLabel?: string; danger?: boolean };
export type PromptOptions = { title: string; label: string; initial?: string; confirmLabel?: string; selectStem?: boolean };

type Pending =
  | { kind: "confirm"; options: ConfirmOptions; resolve: (value: boolean) => void }
  | { kind: "prompt"; options: PromptOptions; resolve: (value: string | null) => void };

function PromptBody({ options, onDone }: { options: PromptOptions; onDone: (value: string | null) => void }) {
  const [value, setValue] = useState(options.initial ?? "");
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    const dot = value.lastIndexOf(".");
    if (options.selectStem && dot > 0) input.setSelectionRange(0, dot); else input.select();
    // Selecting only on mount is intended.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <Modal
      title={options.title}
      onClose={() => onDone(null)}
      actions={<>
        <button type="button" className="btn" onClick={() => onDone(null)}>Cancel</button>
        <button type="button" className="btn btn-primary" disabled={!value.trim()} onClick={() => onDone(value)}>{options.confirmLabel ?? "Save"}</button>
      </>}
    >
      <label className="field">
        <span>{options.label}</span>
        <input ref={inputRef} data-autofocus className="input" value={value} onChange={e => setValue(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && value.trim()) onDone(value); }} spellCheck={false} />
      </label>
    </Modal>
  );
}

/** Promise-based confirm and prompt dialogs rendered inside the app (native ones aren't available on every platform). */
export function useDialogs() {
  const [pending, setPending] = useState<Pending | null>(null);
  const confirm = useCallback((options: ConfirmOptions) => new Promise<boolean>(resolve => setPending({ kind: "confirm", options, resolve })), []);
  const prompt = useCallback((options: PromptOptions) => new Promise<string | null>(resolve => setPending({ kind: "prompt", options, resolve })), []);

  let element: ReactNode = null;
  if (pending?.kind === "confirm") {
    const done = (value: boolean) => { setPending(null); pending.resolve(value); };
    const { options } = pending;
    element = (
      <Modal
        title={options.title}
        onClose={() => done(false)}
        actions={<>
          <button type="button" className="btn" onClick={() => done(false)}>{options.cancelLabel ?? "Cancel"}</button>
          <button type="button" data-autofocus className={`btn ${options.danger ? "btn-danger" : "btn-primary"}`} onClick={() => done(true)}>{options.confirmLabel ?? "OK"}</button>
        </>}
      >
        {typeof options.body === "string" ? <p>{options.body}</p> : options.body}
      </Modal>
    );
  } else if (pending?.kind === "prompt") {
    const done = (value: string | null) => { setPending(null); pending.resolve(value); };
    element = <PromptBody options={pending.options} onDone={done} />;
  }
  return { confirm, prompt, element, open: pending !== null };
}
