import { useEffect, useState } from "react";
import { TweaksView, api, errorText } from "../api";

interface FixesSectionProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** Fixes for things aggressive configs hide; each one is written to gameinfo.gi right away. */
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
      onNotify("ok", on ? "Fix added to gameinfo.gi." : "Fix removed from gameinfo.gi.");
    } catch (e) {
      onNotify("error", errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return null;
  return (
    <fieldset className="field">
      <legend className="field-title">Something missing in game?</legend>
      {view.fixes.map((f) => (
        <label key={f.id} className="fix-row">
          <span>{f.label}</span>
          <input type="checkbox" checked={f.on} disabled={busy} onChange={(e) => void toggle(f.id, e.target.checked)} />
        </label>
      ))}
    </fieldset>
  );
}
