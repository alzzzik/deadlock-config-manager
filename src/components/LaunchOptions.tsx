import { useEffect, useState } from "react";
import { LaunchView, api, errorText } from "../api";

interface LaunchOptionsProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** Launch flags with some evidence they help; everything else can be typed in the line below. */
const FLAGS = [
  {
    flag: "-condebug",
    label: "Write the game's log (for Performance)",
    hint: "The game saves its console to console.log, including its own FPS report after each match.",
  },
  {
    flag: "+exec autoexec",
    label: "Run autoexec.cfg",
    hint: "Needed for the autoexec.cfg lines some configs include. Harmless if you have no autoexec.cfg.",
  },
  { flag: "-vulkan", label: "Use Vulkan", hint: "DirectX 11 is the default and most stable. Vulkan can run better on some AMD GPUs." },
  {
    flag: "-noreflex",
    label: "Turn off NVIDIA Reflex",
    hint: "Some players get steadier frame times with Reflex off and the driver's Low Latency Mode on Ultra instead.",
  },
];

/** Flags can be several words ("+exec autoexec"); matching is on whole words, ignoring case. */
const words = (s: string) => s.split(/\s+/).filter(Boolean);
const findFlag = (opts: string[], flag: string[]) =>
  opts.findIndex((_, i) => flag.every((f, j) => opts[i + j]?.toLowerCase() === f.toLowerCase()));
const hasFlag = (opts: string, flag: string) => findFlag(words(opts), words(flag)) >= 0;
const withFlag = (opts: string, flag: string, on: boolean) => {
  const list = words(opts);
  const f = words(flag);
  const at = findFlag(list, f);
  if (at >= 0) list.splice(at, f.length);
  return [...list, ...(on ? f : [])].join(" ");
};

/** Deadlock's Steam launch options, edited in localconfig.vdf while Steam is closed. */
export function LaunchOptions({ onNotify }: LaunchOptionsProps) {
  const [view, setView] = useState<LaunchView | null>(null);
  const [text, setText] = useState("");

  const load = () =>
    api.getLaunchOptions().then((v) => {
      setView(v);
      setText(v.options);
    }, (e) => onNotify("error", errorText(e)));

  useEffect(() => {
    void load();
    // Load once when the tab opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const saveOptions = () =>
    api.setLaunchOptions(text).then((v) => {
      setView(v);
      setText(v.options);
      onNotify("ok", "Launch options saved. They apply the next time you start Deadlock from Steam.");
    }, (e) => onNotify("error", errorText(e)));

  if (!view) return null;
  if (view.problem) return <p className="notice is-warn">{view.problem}</p>;
  const changed = text.trim() !== view.options.trim();

  return (
    <div>
      {view.steamRunning && (
        <div className="notice is-warn notice-action">
          <span>Close Steam to change launch options. Steam overwrites them when it exits.</span>
          <button className="btn btn-secondary btn-sm" onClick={() => void load()}>
            Check again
          </button>
        </div>
      )}
      {FLAGS.map((f) => (
        <label key={f.flag} className="tweak-row">
          <input type="checkbox" checked={hasFlag(text, f.flag)} onChange={(e) => setText(withFlag(text, f.flag, e.target.checked))} />
          <span className="tweak-text">
            <span>{f.label}</span>
            <small className="muted">{f.hint}</small>
            <code>{f.flag}</code>
          </span>
        </label>
      ))}
      <label className="text-field">
        <span>All launch options</span>
        <input value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} placeholder="(none)" />
      </label>
      <div className="about-actions">
        <button className="btn btn-primary btn-sm" onClick={() => void saveOptions()} disabled={!changed || view.steamRunning}>
          Save launch options
        </button>
        {changed && (
          <button className="btn btn-quiet btn-sm" onClick={() => setText(view.options)}>
            Undo changes
          </button>
        )}
      </div>
    </div>
  );
}
