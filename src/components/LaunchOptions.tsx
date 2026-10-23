import { useEffect, useState } from "react";
import { api, errorText } from "../api";

interface LaunchOptionsProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** Common Deadlock launch options. `group` options replace each other (only one renderer). */
const OPTIONS: { flag: string; text: string; group?: string }[] = [
  { flag: "-novid", text: "Skips the intro video" },
  { flag: "-high", text: "Starts the game with high CPU priority" },
  { flag: "-dx11", text: "DirectX 11 renderer (the default)", group: "renderer" },
  { flag: "-vulkan", text: "Vulkan renderer. Can run better on some AMD GPUs", group: "renderer" },
  { flag: "-noreflex", text: "Turns off NVIDIA Reflex" },
  { flag: "-nojoy", text: "Turns off controller support" },
  { flag: "-console", text: "Opens the console on start" },
  { flag: "-condebug", text: "Writes console.log, needed for Performance" },
  { flag: "+exec autoexec", text: "Runs autoexec.cfg on start" },
];

const words = (s: string) => s.split(/\s+/).filter(Boolean);
const findFlag = (opts: string[], flag: string[]) =>
  opts.findIndex((_, i) => flag.every((f, j) => opts[i + j]?.toLowerCase() === f.toLowerCase()));
const hasFlag = (line: string, flag: string) => findFlag(words(line), words(flag)) >= 0;
const withFlag = (line: string, flag: string, on: boolean) => {
  const list = words(line);
  const f = words(flag);
  const at = findFlag(list, f);
  if (at >= 0) list.splice(at, f.length);
  return [...list, ...(on ? f : [])].join(" ");
};

/** Builds a launch options line to paste into Steam, starting from the one Steam has now. */
export function LaunchOptions({ onNotify }: LaunchOptionsProps) {
  const [line, setLine] = useState("");

  useEffect(() => {
    api.getLaunchOptions().then((v) => setLine(v.options), () => undefined);
  }, []);

  const toggle = (opt: (typeof OPTIONS)[number]) => {
    const on = !hasFlag(line, opt.flag);
    // Picking a renderer removes the other one.
    const cleared = on && opt.group ? OPTIONS.filter((o) => o.group === opt.group).reduce((l, o) => withFlag(l, o.flag, false), line) : line;
    setLine(withFlag(cleared, opt.flag, on));
  };

  const copy = () =>
    navigator.clipboard.writeText(line.trim()).then(
      () => onNotify("ok", "Copied. Paste it in Steam: right-click Deadlock › Properties › Launch options."),
      (e) => onNotify("error", errorText(e)),
    );

  return (
    <div className="launch-builder">
      {OPTIONS.map((o) => {
        const on = hasFlag(line, o.flag);
        return (
          <div key={o.flag} className="launch-row">
            <code>{o.flag}</code>
            <span className="muted small">{o.text}</span>
            <button className={`launch-add ${on ? "is-on" : ""}`} onClick={() => toggle(o)} aria-pressed={on} aria-label={`${on ? "Remove" : "Add"} ${o.flag}`}>
              {on ? "✓" : "+"}
            </button>
          </div>
        );
      })}
      <div className="launch-result">
        <input value={line} onChange={(e) => setLine(e.target.value)} spellCheck={false} placeholder="Click + to add options" aria-label="Launch options" />
        <button className="btn btn-secondary btn-sm" onClick={() => void copy()} disabled={!line.trim()}>
          Copy
        </button>
      </div>
    </div>
  );
}
