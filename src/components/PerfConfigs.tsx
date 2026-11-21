import { useMemo, useState } from "react";
import { ConfigStats, NumberedSession, PerfSort, PerfView, perfReference, sortPerfStats, timeAgo } from "../api";
import { ChevronIcon } from "./icons";

const SORT_KEY = "dcm.perfSort";
const HIDDEN_KEY = "dcm.perfHidden";

const SORTS: { id: PerfSort; label: string }[] = [
  { id: "played", label: "Most played" },
  { id: "newest", label: "Newest" },
  { id: "oldest", label: "Oldest" },
];

export const fps = (n: number) => Math.round(n).toLocaleString();
const pct = (a: number, b: number) => {
  const d = Math.round(((a - b) / b) * 100);
  return d === 0 ? "same" : `${d > 0 ? "+" : ""}${d}%`;
};

/** Reads a remembered choice; storage can be unavailable, so a default always comes back. */
function readStored<T>(key: string, parse: (raw: string) => T, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : parse(raw);
  } catch {
    return fallback;
  }
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable: the choice still holds until the dialog closes.
  }
}

const isSort = (v: string): v is PerfSort => SORTS.some((s) => s.id === v);

/** The per-config list: sorted, with configs the player hid tucked away, each with its own sessions. */
export function PerfConfigs({ view }: { view: PerfView }) {
  const [sort, setSort] = useState<PerfSort>(() => readStored(SORT_KEY, (r) => (isSort(r) ? r : "played"), "played"));
  const [hidden, setHidden] = useState<string[]>(() => readStored(HIDDEN_KEY, (r) => JSON.parse(r) as string[], []));
  const [showHidden, setShowHidden] = useState(false);

  const pickSort = (s: PerfSort) => {
    setSort(s);
    writeStored(SORT_KEY, s);
  };
  const toggleHidden = (key: string) => {
    const next = hidden.includes(key) ? hidden.filter((k) => k !== key) : [...hidden, key];
    setHidden(next);
    writeStored(HIDDEN_KEY, JSON.stringify(next));
  };

  const sorted = useMemo(() => sortPerfStats(view.stats, sort), [view.stats, sort]);
  const shown = sorted.filter((s) => !hidden.includes(s.configKey));
  const tucked = sorted.filter((s) => hidden.includes(s.configKey));
  const reference = perfReference(shown, view.minSessions);
  /** Bars share one scale so configs compare at a glance. */
  const scale = Math.max(1, ...view.stats.map((s) => s.medianAvgFps));

  const row = (s: ConfigStats, isHidden: boolean) => (
    <PerfRow
      key={s.configKey}
      stats={s}
      scale={scale}
      few={s.sessions < view.minSessions}
      note={compareNote(s, reference, view.minSessions)}
      sessions={view.recent.filter((r) => r.configKey === s.configKey && r.avgFps > 0)}
      hidden={isHidden}
      onToggleHidden={() => toggleHidden(s.configKey)}
    />
  );

  return (
    <div className="perf-configs">
      <div className="perf-tools" role="radiogroup" aria-label="Sort configs">
        <span className="muted small">Sort</span>
        {SORTS.map((s) => (
          <label key={s.id} className={`chip ${sort === s.id ? "is-on" : ""}`}>
            <input type="radio" name="perf-sort" checked={sort === s.id} onChange={() => pickSort(s.id)} />
            {s.label}
          </label>
        ))}
      </div>
      {shown.length > 0 && <ul className="perf-list">{shown.map((s) => row(s, false))}</ul>}
      {tucked.length > 0 && (
        <>
          <button className="btn btn-quiet btn-sm perf-hidden-toggle" onClick={() => setShowHidden((v) => !v)} aria-expanded={showHidden}>
            {showHidden ? "Collapse hidden configs" : `Show ${tucked.length} hidden config${tucked.length === 1 ? "" : "s"}`}
          </button>
          {showHidden && <ul className="perf-list is-hidden">{tucked.map((s) => row(s, true))}</ul>}
        </>
      )}
    </div>
  );
}

function compareNote(s: ConfigStats, reference: ConfigStats | null, minSessions: number): string | null {
  if (s.sessions < minSessions) return `Needs ${minSessions}+ sessions to compare`;
  if (!reference) return null;
  if (s.configKey === reference.configKey) return "The others are compared with this one";
  return `${pct(s.medianAvgFps, reference.medianAvgFps)} avg, ${pct(s.medianP1Fps, reference.medianP1Fps)} 1% low vs ${reference.configName}`;
}

interface PerfRowProps {
  stats: ConfigStats;
  /** FPS that fills a whole bar. */
  scale: number;
  few: boolean;
  note: string | null;
  /** This config's sessions, newest first. */
  sessions: NumberedSession[];
  hidden: boolean;
  onToggleHidden: () => void;
}

/** One config: average FPS and 1% lows as bars on the shared scale, and its sessions on request. */
function PerfRow({ stats: s, scale, few, note, sessions, hidden, onToggleHidden }: PerfRowProps) {
  const [open, setOpen] = useState(false);
  const width = (n: number) => `${Math.max(2, Math.min(100, (n / scale) * 100))}%`;
  return (
    <li className={`perf-row ${few ? "is-few" : ""}`}>
      <div className="perf-name">
        <strong>{s.configName}</strong>
        <span className="muted small">
          {s.sessions} session{s.sessions === 1 ? "" : "s"}, {s.minutes} min, last {timeAgo(s.lastPlayed)}
        </span>
        <span className="spacer" />
        <button className="btn btn-quiet btn-sm" onClick={onToggleHidden}>
          {hidden ? "Unhide" : "Hide"}
        </button>
      </div>
      <div className="perf-bars">
        <span className="perf-label">Avg</span>
        <span className="perf-track">
          <span className="perf-bar" style={{ width: width(s.medianAvgFps) }} />
        </span>
        <span className="perf-num">{fps(s.medianAvgFps)}</span>
        <span className="perf-label">1% low</span>
        <span className="perf-track">
          <span className="perf-bar is-low" style={{ width: width(s.medianP1Fps) }} />
        </span>
        <span className="perf-num">{fps(s.medianP1Fps)}</span>
      </div>
      <div className="perf-foot small muted">
        <button className="perf-sessions-toggle" onClick={() => setOpen((v) => !v)} aria-expanded={open} disabled={sessions.length === 0}>
          <ChevronIcon size={12} className="chev" />
          Sessions
        </button>
        <span>Frame time p99 {s.medianFrameMsP99.toFixed(2)} ms</span>
        <span className="spacer" />
        {note && <span>{note}</span>}
      </div>
      {open && <SessionTable sessions={sessions} />}
    </li>
  );
}

const when = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
};

/** Every counted session of one config, newest first. */
function SessionTable({ sessions }: { sessions: NumberedSession[] }) {
  return (
    <table className="perf-sessions">
      <thead>
        <tr>
          <th>#</th>
          <th>When</th>
          <th>Map</th>
          <th>Length</th>
          <th>Avg</th>
          <th>1% low</th>
          <th>p99</th>
        </tr>
      </thead>
      <tbody>
        {sessions.map((r) => (
          <tr key={`${r.recordedAt}-${r.frames}`}>
            <td className="muted">{r.number}</td>
            <td>{when(r.recordedAt)}</td>
            <td>{r.online ? `Match on ${r.map}` : r.map}</td>
            <td>{Math.round(r.seconds / 60)} min</td>
            <td>{fps(r.avgFps)}</td>
            <td>{fps(r.p1Fps)}</td>
            <td>{r.frameMsP99.toFixed(1)} ms</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
