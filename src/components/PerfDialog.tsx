import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ConfigStats, PerfView, api, errorText, timeAgo } from "../api";
import { Modal } from "./Modal";

interface PerfDialogProps {
  onClose: () => void;
  onOpenLaunchOptions: () => void;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

const fps = (n: number) => Math.round(n).toLocaleString();
const pct = (a: number, b: number) => {
  const d = Math.round(((a - b) / b) * 100);
  return d === 0 ? "same" : `${d > 0 ? "+" : ""}${d}%`;
};

/** Real-game FPS per config, from Deadlock's own performance reports in console.log. */
export function PerfDialog({ onClose, onOpenLaunchOptions, onNotify }: PerfDialogProps) {
  const [view, setView] = useState<PerfView | null>(null);

  const load = () => api.getPerf().then(setView, (e) => onNotify("error", errorText(e)));

  useEffect(() => {
    void load();
    let unlisten: (() => void) | undefined;
    listen("perf-changed", () => void load()).then((u) => (unlisten = u), () => undefined);
    return () => unlisten?.();
    // Loads once and on new reports.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const clear = () =>
    api.clearPerf().then(() => {
      void load();
      onNotify("info", "Performance history cleared.");
    }, (e) => onNotify("error", errorText(e)));

  const enough = (s: ConfigStats) => !!view && s.sessions >= view.minSessions;
  const baseline = view?.stats.find((s) => s !== view.stats[0] && enough(s));

  return (
    <Modal
      title="Performance"
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn btn-quiet" onClick={() => void clear()} disabled={!view || view.stats.length === 0}>
            Clear history
          </button>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      <p className="muted small">
        Deadlock measures its own frame rate and writes a report when a map unloads. The app collects those reports while
        it's open (or in the tray) and groups them by the config you had applied. Numbers come from real games, so they
        vary with fights and heroes: compare configs over several matches.
      </p>
      {view && !view.logging && (
        <div className="notice is-warn notice-action">
          <span>
            The game isn't writing its log yet. Add the <code>-condebug</code> launch option, then play as usual.
          </span>
          <button className="btn btn-secondary btn-sm" onClick={onOpenLaunchOptions}>
            Launch options…
          </button>
        </div>
      )}
      {view && view.stats.length === 0 && (
        <p className="muted">No matches recorded yet. Sessions shorter than {view.minSeconds} seconds are left out.</p>
      )}
      {view && view.stats.length > 0 && (
        <table className="perf-table">
          <thead>
            <tr>
              <th>Config</th>
              <th>Sessions</th>
              <th>Avg FPS</th>
              <th>1% low</th>
              <th>Frame time p99</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {view.stats.map((s, i) => (
              <tr key={s.configKey} className={enough(s) ? "" : "is-few"}>
                <td>
                  {s.configName}
                  <small className="muted"> · {timeAgo(s.lastPlayed)}</small>
                </td>
                <td>
                  {s.sessions} <small className="muted">({s.minutes} min)</small>
                </td>
                <td>{fps(s.medianAvgFps)}</td>
                <td>{fps(s.medianP1Fps)}</td>
                <td>{s.medianFrameMsP99.toFixed(2)} ms</td>
                <td className="muted small">
                  {i === 0 && baseline && enough(s)
                    ? `${pct(s.medianAvgFps, baseline.medianAvgFps)} avg, ${pct(s.medianP1Fps, baseline.medianP1Fps)} 1% low vs ${baseline.configName}`
                    : !enough(s)
                      ? `needs ${view.minSessions}+ sessions to compare`
                      : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {view && view.recent.length > 0 && (
        <details className="perf-recent">
          <summary className="small">Recent sessions</summary>
          <ul>
            {view.recent.map((r) => (
              <li key={r.time + r.frames} className="small">
                <code>{r.time}</code> {r.map} · {Math.round(r.seconds / 60)} min · {fps(r.avgFps)} avg / {fps(r.p1Fps)} 1% low ·{" "}
                <span className="muted">{r.configName}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </Modal>
  );
}
