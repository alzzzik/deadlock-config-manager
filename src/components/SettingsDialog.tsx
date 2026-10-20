import { useState } from "react";
import { Overview } from "../api";
import { AddRepo } from "./AddRepo";
import { ChevronIcon, CloseIcon } from "./icons";
import { Modal } from "./Modal";

interface SettingsDialogProps {
  overview: Overview;
  onClose: () => void;
  onSaveKeys: (keys: string[]) => void;
  onAddRepo: (input: string) => Promise<string | null>;
  onMoveRepo: (repo: string, delta: number) => void;
  onRemoveRepo: (repo: string) => void;
  onOpenBackups: () => void;
  onOpenGithub: (url: string) => void;
}

const KEY_LABELS: Record<string, string> = {
  "setting.defaultres": "Resolution width",
  "setting.defaultresheight": "Resolution height",
  "setting.refreshrate_numerator": "Refresh rate (numerator)",
  "setting.refreshrate_denominator": "Refresh rate (denominator)",
  "setting.monitor_index": "Monitor",
  "setting.fullscreen": "Fullscreen",
  "setting.coop_fullscreen": "Borderless fullscreen",
  "setting.nowindowborder": "No window border",
  "setting.aspectratiomode": "Aspect ratio mode",
};

export function SettingsDialog({ overview, onClose, onSaveKeys, onAddRepo, onMoveRepo, onRemoveRepo, onOpenBackups, onOpenGithub }: SettingsDialogProps) {
  const [keys, setKeys] = useState<string[]>(overview.preservedKeys);
  const toggle = (k: string) => setKeys((cur) => (cur.includes(k) ? cur.filter((x) => x !== k) : [...cur, k]));
  const repos = overview.repos;

  return (
    <Modal
      title="Settings"
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-quiet" onClick={onOpenBackups}>
            Open backups folder
          </button>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => onSaveKeys(keys)}>
            Save
          </button>
        </>
      }
    >
      <section className="field">
        <h3 className="field-title">Pack repositories</h3>
        <p className="muted small">Shown on the main screen in this order. Changes here apply right away.</p>
        <ul className="repo-list">
          {repos.map((r, i) => (
            <li key={r}>
              <code>{r}</code>
              <span className="spacer" />
              <button className="icon-btn" onClick={() => onMoveRepo(r, -1)} disabled={i === 0} aria-label={`Move ${r} up`}>
                <ChevronIcon className="rot-up" />
              </button>
              <button className="icon-btn" onClick={() => onMoveRepo(r, 1)} disabled={i === repos.length - 1} aria-label={`Move ${r} down`}>
                <ChevronIcon className="rot-down" />
              </button>
              <button className="icon-btn danger" onClick={() => onRemoveRepo(r)} aria-label={`Remove ${r}`}>
                <CloseIcon />
              </button>
            </li>
          ))}
        </ul>
        <AddRepo linked={repos} onAdd={onAddRepo} onOpenGithub={onOpenGithub} />
      </section>

      <fieldset className="field">
        <legend className="field-title">Keep from my video.txt</legend>
        <p className="muted small">
          Always kept: {overview.alwaysPreserved.join(", ")}. Ticked settings also keep your value instead of the pack's.
        </p>
        <div className="checks">
          {overview.optionalPreserved.map((k) => (
            <label key={k} className="check">
              <input type="checkbox" checked={keys.includes(k)} onChange={() => toggle(k)} />
              <span>{KEY_LABELS[k] ?? k}</span>
              <code>{k.replace("setting.", "")}</code>
            </label>
          ))}
        </div>
      </fieldset>
    </Modal>
  );
}
