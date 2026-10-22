import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { TweaksView, api, errorText } from "../api";
import { Modal } from "./Modal";

interface TweaksDialogProps {
  /** A pack is applied, so saving can re-apply it right away. */
  canReapply: boolean;
  onClose: () => void;
  onSaved: (reapply: boolean) => void;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

const FILE_FILTERS = [{ name: "Overrides", extensions: ["gi", "txt"] }];
const DEFAULT_FOV = 90;

/** Personal changes kept on top of every pack: FAQ fixes, FOV and custom convar lines. */
export function TweaksDialog({ canReapply, onClose, onSaved, onNotify }: TweaksDialogProps) {
  const [view, setView] = useState<TweaksView | null>(null);
  const [fixes, setFixes] = useState<string[]>([]);
  const [fovOn, setFovOn] = useState(false);
  const [fov, setFov] = useState(DEFAULT_FOV);
  const [custom, setCustom] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = async (v: TweaksView) => {
    setView(v);
    setFixes(v.overrides.fixes);
    setFovOn(v.overrides.fov !== null);
    setFov(v.overrides.fov ?? DEFAULT_FOV);
    setCustom(await api.formatTweakLines(v.overrides.custom));
  };

  useEffect(() => {
    api.getTweaks().then(load, (e) => setError(errorText(e)));
  }, []);

  const toggleFix = (id: string) => setFixes((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  const persist = async (reapply: boolean) => {
    try {
      const entries = await api.parseTweakLines(custom);
      await api.saveTweaks({ fixes, fov: fovOn ? fov : null, custom: entries });
      onSaved(reapply);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const importFile = async () => {
    const path = await open({ title: "Import overrides.gi", filters: FILE_FILTERS });
    if (typeof path !== "string") return;
    try {
      await load(await api.importTweaks(path));
      onNotify("ok", "Imported. Check the lines below, then save.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  const exportFile = async () => {
    const path = await save({ title: "Export tweaks", defaultPath: "overrides.gi", filters: FILE_FILTERS });
    if (!path) return;
    try {
      await persist(false);
      await api.exportTweaks(path);
      onNotify("ok", "Tweaks exported.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <Modal
      title="Your tweaks"
      onClose={onClose}
      wide
      footer={
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
      }
    >
      <p className="muted small">
        Tweaks are added on top of whichever pack you apply and stay when you switch or update packs. They only change
        ConVars, so they never affect ranked matchmaking.
      </p>
      {error && <p className="notice is-bad">{error}</p>}

      {view && (
        <>
          <fieldset className="field">
            <legend className="field-title">Fixes</legend>
            <p className="muted small">Common problems with aggressive configs, from the OptimizationLock FAQ.</p>
            <div className="checks">
              {view.fixes.map((f) => (
                <label key={f.id} className="check">
                  <input type="checkbox" checked={fixes.includes(f.id)} onChange={() => toggleFix(f.id)} />
                  <span>{f.label}</span>
                  <code>{f.lines.join(", ")}</code>
                </label>
              ))}
            </div>
          </fieldset>

          <fieldset className="field">
            <legend className="field-title">Field of view</legend>
            <label className="fov-row">
              <input type="checkbox" checked={fovOn} onChange={(e) => setFovOn(e.target.checked)} />
              <span>Set hero camera FOV</span>
              <input
                className="fov-input"
                type="number"
                min={view.minFov}
                max={view.maxFov}
                value={fov}
                disabled={!fovOn}
                onChange={(e) => setFov(Number(e.target.value) || DEFAULT_FOV)}
                aria-label="Field of view"
              />
              <code>citadel_camera_hero_fov</code>
            </label>
          </fieldset>

          <section className="field">
            <h3 className="field-title">Custom lines</h3>
            <p className="muted small">
              One per line: <code>name value</code> sets a setting, <code>// name</code> turns it off, <code>#</code> starts a
              note. Same format as OptimizationLock's overrides.gi.
            </p>
            <textarea
              className="tweak-lines"
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
              spellCheck={false}
              rows={6}
              placeholder={'fps_max "400"\n// r_farz'}
            />
          </section>
        </>
      )}
    </Modal>
  );
}
