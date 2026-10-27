import { useEffect, useState } from "react";
import { TweaksView, api, errorText } from "../api";

interface FixesSectionProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** Switches for things configs often hide. They start as the applied config has them; a change is written to
 * gameinfo.gi right away. */
export function FixesSection({ onNotify }: FixesSectionProps) {
  const [view, setView] = useState<TweaksView | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.getTweaks().then(setView, () => setView(null));
  }, []);

  const toggle = async (id: string, on: boolean) => {
    setBusy(true);
    try {
      setView(await api.setFix(id, on));
      onNotify("ok", "Saved to gameinfo.gi.");
    } catch (e) {
      onNotify("error", errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return null;
  return (
    <fieldset className="field">
      <legend className="field-title">What you see in game</legend>
      <p className="muted small">Each switch starts as your config has it.</p>
      {view.fixes.map((f) => {
        // Distance and detail settings can't be turned off past the config's own value.
        const locked = f.on && f.configShows && !f.canHide;
        return (
          <label key={f.id} className={`fix-row ${locked ? "is-locked" : ""}`}>
            <span className="tweak-text">
              <span>{f.label}</span>
              {locked && <small className="muted">Your config already shows this.</small>}
            </span>
            <input
              type="checkbox"
              role="switch"
              className="switch"
              checked={f.on}
              disabled={busy || locked}
              onChange={(e) => void toggle(f.id, e.target.checked)}
            />
          </label>
        );
      })}
    </fieldset>
  );
}
