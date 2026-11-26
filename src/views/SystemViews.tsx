import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, Cpu, HardDrive, LoaderCircle, Power, RefreshCw, ShieldCheck, Sparkles, Thermometer, Wifi, XOctagon } from "lucide-react";
import { api, errorText, fmtBytes, type HealthReport, type ProcessEntry, type StartupItem, type SystemSnapshot } from "../api";
import type { ConfirmOptions } from "../components/Dialogs";

function Sparkline({ values, color }: { values: number[]; color: string }) {
  const pts = useMemo(() => {
    if (values.length < 2) return "";
    const w = 160;
    const h = 36;
    return values
      .map((v, i) => {
        const x = (i / (values.length - 1)) * w;
        const y = h - (Math.max(0, Math.min(100, v)) / 100) * (h - 4) - 2;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }, [values]);

  return (
    <svg width={160} height={36} viewBox="0 0 160 36" aria-hidden="true" style={{ display: "block" }}>
      {pts && <polyline fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" points={pts} />}
    </svg>
  );
}

export function HealthView({ trashName, onOpenClean }: { trashName: string; onOpenClean: () => void }) {
  const [report, setReport] = useState<HealthReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [error, setError] = useState("");

  const scan = useCallback(() => {
    setBusy(true);
    setError("");
    setMsg("");
    api.runHealthCheck()
      .then(setReport)
      .catch(e => setError(errorText(e)))
      .finally(() => setBusy(false));
  }, []);

  useEffect(() => { scan(); }, [scan]);

  const applyBoost = async () => {
    setBusy(true);
    setError("");
    try {
      const rules = await api.scanDeepCleanup(false);
      const safeIds = rules.filter(r => r.itemCount > 0 && !r.appRunning && r.badge === "safe").map(r => r.id);
      if (safeIds.length === 0) {
        setMsg("System is already clean.");
      } else {
        const res = await api.cleanRules(safeIds, false);
        setMsg(`Boost complete: moved ${res.trashed} safe junk item(s) to ${trashName}.`);
      }
      const updated = await api.runHealthCheck();
      setReport(updated);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Health Overview · 5-Pillar System Check</div>
          <h1 className="big">{report ? `${report.score} / 100` : busy ? "Checking…" : "—"}</h1>
          <p>{msg || (report ? `${fmtBytes(report.totalReclaimable)} safe to reclaim · Local-first diagnostics` : "Scanning junk, startup impact, memory, and disk health")}</p>
        </div>
        <div className="actions">
          <button type="button" className="btn" onClick={scan} disabled={busy}>{busy ? <LoaderCircle size={15} className="spin" /> : <RefreshCw size={15} />} Recheck</button>
          <button type="button" className="btn btn-dark" onClick={() => void applyBoost()} disabled={busy || !report?.totalReclaimable}>
            <Sparkles size={15} /> 1-Click Safe Boost
          </button>
        </div>
      </header>
      {error && <div className="state error"><h3>Health check failed</h3><p>{error}</p></div>}
      {report && (
        <section className="panel">
          <h2><span>Diagnostic Pillars</span><button type="button" className="btn btn-sm" onClick={onOpenClean}>Review Deep Clean</button></h2>
          {report.pillars.map(p => (
            <div key={p.id} className="panel-row">
              <ShieldCheck size={18} aria-hidden="true" />
              <span className="grow">
                <strong style={{ display: "block" }}>{p.title}</strong>
                <span className="sub">{p.summary}</span>
              </span>
              <span className={`pill ${p.status === "good" ? "live" : ""}`}>
                {p.status === "good" ? "Good" : p.status === "review" ? "Review" : "Attention"}
              </span>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}

export function MonitorView() {
  const [snap, setSnap] = useState<SystemSnapshot | null>(null);
  const [cpuHist, setCpuHist] = useState<number[]>([]);
  const [memHist, setMemHist] = useState<number[]>([]);

  useEffect(() => {
    let alive = true;
    const tick = () => {
      void api.systemSnapshot().then(s => {
        if (!alive) return;
        setSnap(s);
        const memPct = s.memTotal > 0 ? (s.memUsed / s.memTotal) * 100 : 0;
        setCpuHist(h => [...h.slice(-29), s.cpuUsage]);
        setMemHist(h => [...h.slice(-29), memPct]);
      }).catch(() => undefined);
    };
    tick();
    const id = window.setInterval(tick, 1500);
    return () => { alive = false; window.clearInterval(id); };
  }, []);

  if (!snap) {
    return <div className="state"><LoaderCircle size={22} className="spin" /><span>Reading hardware sensors…</span></div>;
  }
  const memPct = snap.memTotal > 0 ? Math.round((snap.memUsed / snap.memTotal) * 100) : 0;

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">{snap.osName} · Kernel {snap.kernelVersion}</div>
          <h1 className="big">{snap.cpuBrand || snap.hostName}</h1>
          <p>{snap.coreCount} logical cores · Uptime {Math.floor(snap.uptimeSecs / 3600)}h {Math.floor((snap.uptimeSecs % 3600) / 60)}m</p>
        </div>
      </header>

      <div className="columns">
        <section className="panel" style={{ flex: 1 }}>
          <h2><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Cpu size={15} /> CPU · {snap.cpuUsage.toFixed(1)}%</span><Sparkline values={cpuHist} color="var(--chart-1)" /></h2>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(120px, 1fr))", gap: 8, padding: 12 }}>
            {snap.cores.slice(0, 16).map(core => (
              <div key={core.name} style={{ fontSize: 12 }}>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span className="muted">{core.name}</span>
                  <span className="num">{core.usage.toFixed(0)}%</span>
                </div>
                <span className="meter" style={{ display: "block", marginTop: 4 }}><i style={{ width: `${Math.min(100, core.usage)}%` }} /></span>
              </div>
            ))}
          </div>
        </section>

        <section className="panel" style={{ flex: 1 }}>
          <h2><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><HardDrive size={15} /> Memory · {memPct}%</span><Sparkline values={memHist} color="var(--chart-2)" /></h2>
          <div className="panel-row"><span className="grow">Used RAM</span><span className="num">{fmtBytes(snap.memUsed)} / {fmtBytes(snap.memTotal)}</span></div>
          <div className="panel-row"><span className="grow">Available RAM</span><span className="num">{fmtBytes(snap.memAvailable)}</span></div>
          <div className="panel-row"><span className="grow">Swap</span><span className="num">{fmtBytes(snap.swapUsed)} / {fmtBytes(snap.swapTotal)}</span></div>
        </section>
      </div>

      <div className="columns" style={{ marginTop: 12 }}>
        <section className="panel" style={{ flex: 1 }}>
          <h2><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Wifi size={15} /> Network Interfaces</span></h2>
          {snap.networks.slice(0, 6).map(n => (
            <div key={n.name} className="panel-row">
              <span className="grow truncate">{n.name}</span>
              <span className="muted small">↓ {fmtBytes(n.receivedBytes)} · ↑ {fmtBytes(n.transmittedBytes)}</span>
            </div>
          ))}
        </section>
        <section className="panel" style={{ flex: 1 }}>
          <h2><span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Thermometer size={15} /> Temperatures</span></h2>
          {snap.temperatures.length === 0 ? (
            <div className="state"><p>No hardware thermal sensors exposed without kernel drivers.</p></div>
          ) : snap.temperatures.slice(0, 8).map(t => (
            <div key={t.label} className="panel-row">
              <span className="grow truncate">{t.label}</span>
              <span className="num">{t.celsius.toFixed(1)} °C</span>
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}

export function StartupView() {
  const [items, setItems] = useState<StartupItem[] | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    setError("");
    api.listStartupItems().then(setItems).catch(e => setError(errorText(e)));
  }, []);
  useEffect(() => { load(); }, [load]);

  const toggle = async (item: StartupItem) => {
    try {
      await api.toggleStartupItem(item.id, !item.enabled);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Startup Apps · User Autostart Entries</div>
          <h1 className="big">{items ? `${items.filter(i => i.enabled).length} enabled` : "—"}</h1>
          <p>Disable unneeded startup apps to speed up sign-in across Windows, macOS, and Linux.</p>
        </div>
        <button type="button" className="btn" onClick={load}><RefreshCw size={15} /> Refresh</button>
      </header>
      {error && <div className="state error"><h3>Startup error</h3><p>{error}</p></div>}
      <section className="panel">
        {items?.length === 0 && <div className="state"><p>No user startup items found.</p></div>}
        {items?.map(item => (
          <div key={item.id} className="panel-row">
            <Power size={16} aria-hidden="true" />
            <span className="grow" style={{ minWidth: 0 }}>
              <strong className="truncate" style={{ display: "block" }}>{item.name}</strong>
              <span className="sub truncate">{item.scope} · {item.command}</span>
            </span>
            <span className={`pill ${item.enabled ? "live" : ""}`}>{item.enabled ? "Enabled" : "Disabled"}</span>
            <button type="button" className="btn btn-sm" onClick={() => void toggle(item)}>{item.enabled ? "Disable" : "Enable"}</button>
          </div>
        ))}
      </section>
    </div>
  );
}

export function ProcessView({ confirm }: { confirm: (o: ConfirmOptions) => Promise<boolean> }) {
  const [procs, setProcs] = useState<ProcessEntry[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");

  const load = useCallback(() => {
    api.listProcesses().then(setProcs).catch(e => setError(errorText(e)));
  }, []);
  useEffect(() => {
    load();
    const id = window.setInterval(load, 2500);
    return () => window.clearInterval(id);
  }, [load]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return procs.filter(p => !q || p.name.toLowerCase().includes(q) || String(p.pid).includes(q)).slice(0, 120);
  }, [procs, query]);

  const endProc = async (p: ProcessEntry) => {
    if (p.protected) return;
    const ok = await confirm({
      title: `End “${p.name}” (PID ${p.pid})?`,
      body: "Any unsaved work in this application may be lost.",
      confirmLabel: "End Process",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.terminateProcess(p.pid);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="tool">
      <header className="tool-head">
        <div>
          <div className="eyebrow">Process Manager · Live Activity</div>
          <h1 className="big">{procs.length} processes</h1>
          <p>Critical OS processes are protected from accidental termination.</p>
        </div>
        <label className="filter-box">
          <Activity size={14} aria-hidden="true" />
          <input value={query} onChange={e => setQuery(e.target.value)} placeholder="Filter name or PID" />
        </label>
      </header>
      {error && <div className="state error"><h3>Process error</h3><p>{error}</p></div>}
      <section className="panel">
        {filtered.map(p => (
          <div key={p.pid} className="panel-row" title={p.exePath || p.name}>
            <span className="grow" style={{ minWidth: 0 }}>
              <strong className="truncate" style={{ display: "block" }}>{p.name} <span className="muted small">#{p.pid}</span></strong>
              <span className="sub truncate">{p.exePath || p.status}</span>
            </span>
            <span className="num" style={{ width: 64, textAlign: "right" }}>{p.cpuUsage.toFixed(1)}%</span>
            <span className="num" style={{ width: 82, textAlign: "right" }}>{fmtBytes(p.memoryBytes)}</span>
            {p.protected ? (
              <span className="pill">Protected</span>
            ) : (
              <button type="button" className="btn btn-sm danger" onClick={() => void endProc(p)}><XOctagon size={13} /> End</button>
            )}
          </div>
        ))}
      </section>
    </div>
  );
}
