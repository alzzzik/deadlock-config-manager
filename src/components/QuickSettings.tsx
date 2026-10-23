import { useEffect, useState } from "react";
import { TweakRow, TweaksView, api, errorText } from "../api";

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
      <span className="qs-desc">{row.description}</span>
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

/** View width and FPS caps, editable in place; a change is written to gameinfo.gi right away. */
export function QuickSettings({ refreshKey, disabled, onApplied, onError }: QuickSettingsProps) {
  const [view, setView] = useState<TweaksView | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api.getTweaks().then(setView, () => setView(null));
  }, [refreshKey]);

  const save = async (id: string, value: string | null) => {
    setSaving(true);
    try {
      setView(await api.setTweak(id, value));
      onApplied("Saved to gameinfo.gi.");
    } catch (e) {
      onError(errorText(e));
      api.getTweaks().then(setView, () => undefined);
    } finally {
      setSaving(false);
    }
  };

  if (!view) return null;
  return (
    <section className="quick-settings" aria-label="Your settings">
      {view.rows.map((r) => (
        <Row key={r.id} row={r} disabled={disabled || saving} onSave={(v) => void save(r.id, v)} />
      ))}
    </section>
  );
}
