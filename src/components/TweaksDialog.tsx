import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { TweakToggle, TweaksView, api, errorText } from "../api";
import { Modal } from "./Modal";
import { LaunchOptions } from "./LaunchOptions";

interface TweaksDialogProps {
  /** A pack is applied, so saving can re-apply it right away. */
  canReapply: boolean;
  onClose: () => void;
  onSaved: (reapply: boolean) => void;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

const FILE_FILTERS = [{ name: "Overrides", extensions: ["gi", "txt"] }];
/** Common caps: a high cap in matches keeps frame times even; menus don't need many frames. */
const DEFAULT_FPS = 400;
const DEFAULT_FPS_UI = 120;
/** Select value meaning "keep whatever the pack sets". */
const PACK_VALUE = "";
const CUSTOM = "custom";

interface CapRowProps {
  label: string;
  hint: string;
  value: number | null;
  fallback: number;
  max: number;
  onChange: (v: number | null) => void;
}

/** "Keep the pack's value" or a number, for fps_max / fps_max_ui. */
function CapRow({ label, hint, value, fallback, max, onChange }: CapRowProps) {
  return (
    <label className="tweak-row">
      <input type="checkbox" checked={value !== null} onChange={(e) => onChange(e.target.checked ? fallback : null)} />
      <span className="tweak-text">
        <span>{label}</span>
        <small className="muted">{hint}</small>
      </span>
      <input
        className="num-input"
        type="number"
        min={0}
        max={max}
        value={value ?? fallback}
        disabled={value === null}
        onChange={(e) => onChange(Math.max(0, Math.min(max, Number(e.target.value) || 0)))}
        aria-label={label}
      />
    </label>
  );
}

function ToggleRow({ t, on, onToggle }: { t: TweakToggle; on: boolean; onToggle: () => void }) {
  return (
    <label className="tweak-row">
      <input type="checkbox" checked={on} onChange={onToggle} />
      <span className="tweak-text">
        <span>{t.label}</span>
        {t.hint && <small className="muted">{t.hint}</small>}
        <code>{t.lines.join(", ")}</code>
      </span>
    </label>
  );
}

/** Personal changes kept on top of every pack. */
export function TweaksDialog({ canReapply, onClose, onSaved, onNotify }: TweaksDialogProps) {
  const [view, setView] = useState<TweaksView | null>(null);
  const [fixes, setFixes] = useState<string[]>([]);
  const [aspect, setAspect] = useState<string>(PACK_VALUE);
  const [customAspect, setCustomAspect] = useState("2.15");
  const [fps, setFps] = useState<number | null>(null);
  const [fpsUi, setFpsUi] = useState<number | null>(null);
  const [custom, setCustom] = useState("");
  const [showFixes, setShowFixes] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"config" | "launch">("config");

  const load = async (v: TweaksView) => {
    setView(v);
    const o = v.overrides;
    setFixes(o.fixes);
    const preset = v.aspectPresets.find((p) => p.value === o.aspect);
    setAspect(o.aspect === null ? PACK_VALUE : preset ? preset.value : CUSTOM);
    if (o.aspect !== null && !preset) setCustomAspect(o.aspect);
    setFps(o.fpsMax);
    setFpsUi(o.fpsMaxUi);
    setShowFixes(v.toggles.some((t) => t.group === "fix" && o.fixes.includes(t.id)));
    setCustom(await api.formatTweakLines(o.custom));
  };

  useEffect(() => {
    api.getTweaks().then(load, (e) => setError(errorText(e)));
  }, []);

  const toggle = (id: string) => setFixes((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  const persist = async (reapply: boolean): Promise<boolean> => {
    try {
      const entries = await api.parseTweakLines(custom);
      const aspectValue = aspect === PACK_VALUE ? null : aspect === CUSTOM ? customAspect : aspect;
      await api.saveTweaks({ fixes, aspect: aspectValue, fpsMax: fps, fpsMaxUi: fpsUi, custom: entries });
      onSaved(reapply);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    }
  };

  const importFile = async () => {
    const path = await open({ title: "Import overrides.gi", filters: FILE_FILTERS });
    if (typeof path !== "string") return;
    try {
      await load(await api.importTweaks(path));
      onNotify("ok", "Imported into Custom lines. Check them, then save.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  const exportFile = async () => {
    const path = await save({ title: "Export tweaks", defaultPath: "overrides.gi", filters: FILE_FILTERS });
    if (!path) return;
    try {
      const entries = await api.parseTweakLines(custom);
      const aspectValue = aspect === PACK_VALUE ? null : aspect === CUSTOM ? customAspect : aspect;
      await api.saveTweaks({ fixes, aspect: aspectValue, fpsMax: fps, fpsMaxUi: fpsUi, custom: entries });
      await api.exportTweaks(path);
      onNotify("ok", "Tweaks saved and exported.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  const feel = view?.toggles.filter((t) => t.group === "feel") ?? [];
  const fixList = view?.toggles.filter((t) => t.group === "fix") ?? [];
  const fixesOn = fixList.filter((t) => fixes.includes(t.id)).length;

  return (
    <Modal
      title="Your tweaks"
      onClose={onClose}
      wide
      footer={
        tab === "launch" ? (
          <>
            <span className="spacer" />
            <button className="btn btn-secondary" onClick={onClose}>
              Close
            </button>
          </>
        ) : (
        <>
          <button className="btn btn-quiet" onClick={importFile}>
            Import…
          </button>
          <button className="btn btn-quiet" onClick={exportFile}>
            Export…
          </button>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onClose}>
            Cancel
          </button>
          {canReapply ? (
            <>
              <button className="btn btn-secondary" onClick={() => void persist(false)} disabled={!view}>
                Save
              </button>
              <button className="btn btn-primary" onClick={() => void persist(true)} disabled={!view}>
                Save and re-apply
              </button>
            </>
          ) : (
            <button className="btn btn-primary" onClick={() => void persist(false)} disabled={!view}>
              Save
            </button>
          )}
        </>
        )
      }
    >
      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "config"} className={tab === "config" ? "is-on" : ""} onClick={() => setTab("config")}>
          Config tweaks
        </button>
        <button role="tab" aria-selected={tab === "launch"} className={tab === "launch" ? "is-on" : ""} onClick={() => setTab("launch")}>
          Launch options
        </button>
      </div>
      {tab === "launch" && <LaunchOptions onNotify={onNotify} />}
      {tab === "config" && (
      <>
      <p className="muted small">
        Tweaks go on top of whichever config you apply and stay when you switch or update configs. They only change
        ConVars, so they never affect ranked matchmaking. Anything left unset keeps the config's own value.
      </p>
      {error && <p className="notice is-bad">{error}</p>}

      {view && (
        <>
          <section className="field">
            <h3 className="field-title">View width</h3>
            <p className="muted small">
              How wide you see, set with <code>r_aspectratio</code> the way most configs do it. Higher shows more around
              you; heroes look a bit smaller.
            </p>
            <div className="chip-row" role="radiogroup" aria-label="View width">
              {[{ label: "Config's value", value: PACK_VALUE }, ...view.aspectPresets, { label: "Custom", value: CUSTOM }].map((p) => (
                <label key={p.label} className={`chip ${aspect === p.value ? "is-on" : ""}`}>
                  <input type="radio" name="aspect" checked={aspect === p.value} onChange={() => setAspect(p.value)} />
                  {p.label}
                </label>
              ))}
              {aspect === CUSTOM && (
                <input
                  className="num-input"
                  value={customAspect}
                  onChange={(e) => setCustomAspect(e.target.value)}
                  aria-label="Custom r_aspectratio"
                  inputMode="decimal"
                />
              )}
            </div>
          </section>

          <section className="field">
            <h3 className="field-title">Frame rate</h3>
            <CapRow
              label="Cap FPS in matches"
              hint="fps_max. 0 means no cap. A cap a bit above what your PC holds keeps frame times even."
              value={fps}
              fallback={DEFAULT_FPS}
              max={view.maxFps}
              onChange={setFps}
            />
            <CapRow
              label="Cap FPS in menus"
              hint="fps_max_ui. A low cap keeps the GPU cool and quiet in menus."
              value={fpsUi}
              fallback={DEFAULT_FPS_UI}
              max={view.maxFps}
              onChange={setFpsUi}
            />
          </section>

          <section className="field">
            <h3 className="field-title">Gameplay feel</h3>
            {feel.map((t) => (
              <ToggleRow key={t.id} t={t} on={fixes.includes(t.id)} onToggle={() => toggle(t.id)} />
            ))}
          </section>

          <section className="field">
            <button className="field-toggle" onClick={() => setShowFixes((v) => !v)} aria-expanded={showFixes}>
              <h3 className="field-title">Visibility fixes</h3>
              <span className="muted small">
                {fixesOn > 0 ? `${fixesOn} on` : "For things aggressive configs hide"} · {showFixes ? "Hide" : "Show"}
              </span>
            </button>
            {showFixes &&
              fixList.map((t) => <ToggleRow key={t.id} t={t} on={fixes.includes(t.id)} onToggle={() => toggle(t.id)} />)}
          </section>

          <section className="field">
            <h3 className="field-title">Custom lines</h3>
            <p className="muted small">
              One per line: <code>name value</code> sets a setting, <code>// name</code> turns it off, <code>#</code> starts a
              note. Same format as OptimizationLock's overrides.gi. These win over everything above.
            </p>
            <textarea
              className="tweak-lines"
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
              spellCheck={false}
              rows={4}
              placeholder={'citadel_unit_status_dpi "6"\n// r_farz'}
            />
          </section>
        </>
      )}
      </>
      )}
    </Modal>
  );
}
