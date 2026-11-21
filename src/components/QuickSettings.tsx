import { useEffect, useState } from "react";
import { FixRow, TweakRow, TweaksView, api, errorText, fovFor } from "../api";

interface QuickSettingsProps {
  /** Bumped after an apply/restore so the config's values are re-read. */
  refreshKey: string;
  disabled: boolean;
  onApplied: (message: string) => void;
  onError: (message: string) => void;
}

function Row({ row, disabled, onSave }: { row: TweakRow; disabled: boolean; onSave: (value: string | null) => void }) {
  const shown = row.value ?? row.configValue ?? "";
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    const t = text.trim();
    if (t === shown) return;
    onSave(t === "" || t === row.configValue ? null : t);
  };
  return (
    <div className="qs-row">
      <span className="qs-label">{row.label}</span>
      <span className="qs-desc">
        {row.id === "aspect" && fovFor(parseFloat(text.replace(",", "."))) !== null && (
          <strong className="qs-fov">≈ {fovFor(parseFloat(text.replace(",", ".")))} FOV · </strong>
        )}
        {row.description}
      </span>
      <input
        className="qs-input"
        value={text}
        placeholder={row.configValue ?? "not set"}
        disabled={disabled}
        inputMode="decimal"
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        aria-label={row.label}
      />
      {row.value !== null ? (
        <button className="qs-reset" disabled={disabled} onClick={() => onSave(null)} title={`Back to the config's value (${row.configValue ?? "not set"})`}>
          Reset
        </button>
      ) : (
        <span className="qs-reset" aria-hidden />
      )}
    </div>
  );
}

/** A switch for something configs often hide. Distance and detail settings can't be turned off past the
 * config's own value, so those are locked on. */
function FixSwitch({ fix: f, disabled, onToggle }: { fix: FixRow; disabled: boolean; onToggle: (on: boolean) => void }) {
  const locked = f.on && f.configShows && !f.canHide;
  return (
    <label className={`fix-row ${locked ? "is-locked" : ""}`} title={locked ? "Your config already shows this." : undefined}>
      <span>{f.label}</span>
      <input
        type="checkbox"
        role="switch"
        className="switch"
        checked={f.on}
        disabled={disabled || locked}
        onChange={(e) => onToggle(e.target.checked)}
      />
    </label>
  );
}

/** View width, FPS caps and the "what you see in game" switches, edited in place; each change is written to
 * gameinfo.gi right away. */
export function QuickSettings({ refreshKey, disabled, onApplied, onError }: QuickSettingsProps) {
  const [view, setView] = useState<TweaksView | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api.getTweaks().then(setView, () => setView(null));
  }, [refreshKey]);

  const run = async (change: Promise<TweaksView>) => {
    setSaving(true);
    try {
      setView(await change);
      onApplied("Saved to gameinfo.gi.");
    } catch (e) {
      onError(errorText(e));
      api.getTweaks().then(setView, () => undefined);
    } finally {
      setSaving(false);
    }
  };

  if (!view) return null;
  const off = disabled || saving;
  return (
    <section className="your-settings" aria-labelledby="your-settings-title">
      <div className="section-head">
        <h2 id="your-settings-title">Your settings</h2>
        <span className="hint">Written to gameinfo.gi as soon as you change them</span>
      </div>
      <div className="quick-settings">
        {view.rows.map((r) => (
          <Row key={r.id} row={r} disabled={off} onSave={(v) => void run(api.setTweak(r.id, v))} />
        ))}
        {view.fixes.length > 0 && (
          <div className="fixes">
            <div className="fixes-head">
              <h3>What you see in game</h3>
              <span className="hint">Each switch starts as your config has it</span>
            </div>
            <div className="fix-grid">
              {view.fixes.map((f) => (
                <FixSwitch key={f.id} fix={f} disabled={off} onToggle={(on) => void run(api.setFix(f.id, on))} />
              ))}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
