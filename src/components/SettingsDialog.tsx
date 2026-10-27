import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Automation, ISSUES_URL, InstallKind, Overview, api, errorText } from "../api";
import { AddRepo } from "./AddRepo";
import { ChevronIcon, CloseIcon } from "./icons";
import { Modal } from "./Modal";
import { UpdateCheck } from "./AppUpdate";
import { FixesSection } from "./FixesSection";
import { LaunchOptions } from "./LaunchOptions";

interface SettingsDialogProps {
  overview: Overview;
  onClose: () => void;
  onSaveKeys: (keys: string[]) => void;
  onAddRepo: (input: string) => Promise<string | null>;
  onBrowse: () => void;
  onMoveRepo: (repo: string, delta: number) => void;
  onRemoveRepo: (repo: string) => void;
  onOpenBackups: () => void;
  onOpenGithub: (url: string) => void;
  onCheckUpdate: () => Promise<UpdateCheck>;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
  installKind: InstallKind | null;
  onShowGuide: () => void;
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

export function SettingsDialog({
  overview,
  onClose,
  onSaveKeys,
  onAddRepo,
  onBrowse,
  onMoveRepo,
  onRemoveRepo,
  onOpenBackups,
  onOpenGithub,
  onCheckUpdate,
  onNotify,
  installKind,
  onShowGuide,
}: SettingsDialogProps) {
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
          <button className="btn btn-quiet" onClick={onShowGuide}>
            Starting config guide
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
        <AddRepo linked={repos} onAdd={onAddRepo} onOpenGithub={onOpenGithub} onBrowse={onBrowse} />
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

      <FixesSection onNotify={onNotify} />

      <fieldset className="field">
        <legend className="field-title">Launch options</legend>
        <LaunchOptions onNotify={onNotify} />
      </fieldset>

      <GameUpdatesSection onNotify={onNotify} />

      <AboutSection onOpenGithub={onOpenGithub} onCheckUpdate={onCheckUpdate} onNotify={onNotify} installKind={installKind} />
    </Modal>
  );
}

interface AboutSectionProps {
  onOpenGithub: (url: string) => void;
  onCheckUpdate: () => Promise<UpdateCheck>;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
  installKind: InstallKind | null;
}

const RELEASES_URL = "https://github.com/alzzzik/deadlock-config-manager/releases";

const UPDATE_NOTE: Record<Exclude<InstallKind, "installer">, string> = {
  scoop: "Installed with Scoop: update with scoop update deadlock-config-manager.",
  portable: "Portable copy: download new versions from GitHub releases.",
};

/** App version, update check and the diagnostics report for bug reports. */
function AboutSection({ onOpenGithub, onCheckUpdate, onNotify, installKind }: AboutSectionProps) {
  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion, () => setVersion(null));
  }, []);

  const check = async () => {
    setChecking(true);
    const result = await onCheckUpdate();
    setChecking(false);
    if (result === "none") onNotify("ok", "You have the latest version.");
    if (result === "failed") onNotify("error", "Couldn't reach GitHub to check for updates. Try again later.");
  };

  const copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(await api.diagnostics());
      onNotify("ok", "Diagnostics copied. Paste them into your bug report.");
    } catch (e) {
      onNotify("error", `Couldn't copy diagnostics: ${errorText(e)}`);
    }
  };

  return (
    <section className="field">
      <h3 className="field-title">About</h3>
      <p className="muted small">
        Deadlock Config Manager {version ?? ""}. Diagnostics list the app and Windows version, the detected paths
        (with your user name removed) and recent log lines. Nothing is sent automatically.
      </p>
      {installKind && installKind !== "installer" && <p className="muted small">{UPDATE_NOTE[installKind]}</p>}
      <div className="about-actions">
        {installKind === "installer" ? (
          <button className="btn btn-secondary btn-sm" onClick={check} disabled={checking}>
            {checking ? "Checking…" : "Check for updates"}
          </button>
        ) : (
          <button className="btn btn-secondary btn-sm" onClick={() => onOpenGithub(RELEASES_URL)}>
            Releases
          </button>
        )}
        <button className="btn btn-secondary btn-sm" onClick={copyDiagnostics}>
          Copy diagnostics
        </button>
        <button className="btn btn-quiet btn-sm" onClick={() => onOpenGithub(ISSUES_URL)}>
          Report a problem
        </button>
      </div>
    </section>
  );
}

/** Overlay mode and hands-off re-apply; changes save right away. */
function GameUpdatesSection({ onNotify }: { onNotify: (kind: "ok" | "error" | "info", text: string) => void }) {
  const [a, setA] = useState<Automation | null>(null);

  useEffect(() => {
    api.getAutomation().then(setA, () => setA(null));
  }, []);

  const change = (next: Automation) => {
    setA(next);
    api.setAutomation(next).then(setA, (e) => onNotify("error", errorText(e)));
  };

  if (!a) return null;
  return (
    <fieldset className="field">
      <legend className="field-title">Game updates</legend>
      <label className="tweak-row">
        <input type="checkbox" checked={a.autoReapply} onChange={(e) => change({ ...a, autoReapply: e.target.checked })} />
        <span className="tweak-text">
          <span>Re-apply my config after game updates</span>
          <small className="muted">
            When an update replaces gameinfo.gi, the app applies your config again by itself (while Deadlock is closed) and
            shows a notification. Closing the window keeps the app in the tray so it can do this.
          </small>
        </span>
      </label>
      <label className="tweak-row">
        <input
          type="checkbox"
          checked={a.startWithWindows}
          onChange={(e) => change({ ...a, startWithWindows: e.target.checked })}
        />
        <span className="tweak-text">
          <span>Start with Windows in the tray</span>
          <small className="muted">
            Closing the window keeps the app in the tray (right-click its icon › Quit to exit), so performance reports are
            collected even if you never open it.
          </small>
        </span>
      </label>
      <label className="tweak-row">
        <input type="checkbox" checked={a.overlay} onChange={(e) => change({ ...a, overlay: e.target.checked })} />
        <span className="tweak-text">
          <span>Keep settings Valve adds in updates (overlay)</span>
          <small className="muted">
            Applies configs on top of Valve's current gameinfo.gi, so settings a newer game version added aren't removed. The
            config still wins for everything it sets. Off applies the config's file exactly.
          </small>
        </span>
      </label>
    </fieldset>
  );
}

