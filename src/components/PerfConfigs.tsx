import { useMemo, useState } from "react";
import { ConfigStats, NumberedSession, PERF_OLDER_DAYS, PerfSort, PerfView, isOlderConfig, perfReference, sortPerfStats, timeAgo } from "../api";
import { ChevronIcon } from "./icons";

/** v2: 0.9.8 made "Best 1% low" the default, so an old remembered default doesn't hide it. */
const SORT_KEY = "dcm.perfSort2";
const HIDDEN_KEY = "dcm.perfHidden";

const SORTS: { id: PerfSort; label: string }[] = [
  { id: "low", label: "Best 1% low" },
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

/** Hidden config keys; anything else in storage (edited or damaged) counts as none hidden. */
function parseKeys(raw: string): string[] {
  const v: unknown = JSON.parse(raw);
  return Array.isArray(v) ? v.filter((k): k is string => typeof k === "string") : [];
}

/** The per-config list: sorted, with configs the player hid tucked away, each with its own sessions. */
export function PerfConfigs({ view }: { view: PerfView }) {
  const [sort, setSort] = useState<PerfSort>(() => readStored(SORT_KEY, (r) => (isSort(r) ? r : "low"), "low"));
  const [hidden, setHidden] = useState<string[]>(() => readStored(HIDDEN_KEY, parseKeys, []));
  const [showHidden, setShowHidden] = useState(false);
  const [showOlder, setShowOlder] = useState(false);

  const pickSort = (s: PerfSort) => {
    setSort(s);
    writeStored(SORT_KEY, s);
  };
  const toggleHidden = (key: string) => {
    const next = hidden.includes(key) ? hidden.filter((k) => k !== key) : [...hidden, key];
    setHidden(next);
    writeStored(HIDDEN_KEY, JSON.stringify(next));
  };

  const sorted = useMemo(() => sortPerfStats(view.stats, sort, view.minSessions), [view.stats, sort, view.minSessions]);
  const visible = sorted.filter((s) => !hidden.includes(s.configKey));
  const shown = visible.filter((s) => !isOlderConfig(s));
  const older = visible.filter((s) => isOlderConfig(s));
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
      {older.length > 0 && (
        <>
          <button
            className="btn btn-quiet btn-sm perf-hidden-toggle"
            onClick={() => setShowOlder((v) => !v)}
            aria-expanded={showOlder}
            title={`Not played in the last ${PERF_OLDER_DAYS} days`}
          >
            {showOlder ? "Collapse older configs" : `Show ${older.length} older config${older.length === 1 ? "" : "s"}`}
          </button>
          {showOlder && <ul className="perf-list is-hidden">{older.map((s) => row(s, false))}</ul>}
        </>
      )}
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

interface CompareNote {
  text: string;
  title?: string;
  /** "up" / "down" colour the tag by the 1% low difference. */
  tone?: "up" | "down";
}

/** A short tag: the 1% low difference first, since that's what decides between configs. */
function compareNote(s: ConfigStats, reference: ConfigStats | null, minSessions: number): CompareNote | null {
  if (s.sessions < minSessions) return { text: `Needs ${minSessions}+ sessions` };
  if (!reference) return null;
  if (s.configKey === reference.configKey) return { text: "Baseline", title: "The others are compared with this one" };
  const low = pct(s.medianP1Fps, reference.medianP1Fps);
  const tone = s.medianP1Fps > reference.medianP1Fps ? "up" : s.medianP1Fps < reference.medianP1Fps ? "down" : undefined;
  return {
    text: `${low} 1% low ·${pct(s.medianAvgFps, reference.medianAvgFps)} avg`,
    title: `Compared with ${reference.configName}`,
    tone,
  };
}

interface PerfRowProps {
  stats: ConfigStats;
  /** FPS that fills a whole bar. */
  scale: number;
  few: boolean;
  note: CompareNote | null;
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
        <button className="perf-sessions-toggle" onClick={() => setOpen((v) => !v)} aria-expanded={open} disabled={sessions.length === 0}>
          <ChevronIcon size={12} className="chev" />
          {s.sessions} session{s.sessions === 1 ? "" : "s"}
        </button>
        <span className="spacer" />
        {note && (
          <span className={`perf-tag ${note.tone ? `is-${note.tone}` : ""}`} title={note.title}>
            {note.text}
          </span>
        )}
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
      {open && (
        <>
          <p className="perf-detail small muted">
            {s.minutes} min played, last {timeAgo(s.lastPlayed)} · median frame time p99 {s.medianFrameMsP99.toFixed(2)} ms
          </p>
          <SessionTable sessions={sessions} />
        </>
      )}
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
