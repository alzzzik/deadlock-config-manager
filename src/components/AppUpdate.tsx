import { useCallback, useEffect, useState } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { InstallKind, api, errorText } from "../api";

/** How often the app checks GitHub for a newer version of itself while open. */
const APP_UPDATE_CHECK_MS = 6 * 60 * 60 * 1000;

export type UpdateCheck = "found" | "none" | "failed";

export type AppUpdateState =
  | { kind: "idle" }
  | { kind: "available"; update: Update }
  | { kind: "installing"; version: string; percent: number | null }
  | { kind: "failed"; version: string; error: string };

/** Checks for a new app version on startup and every few hours; failures stay silent. */
export function useAppUpdate() {
  const [state, setState] = useState<AppUpdateState>({ kind: "idle" });
  const [installKind, setInstallKind] = useState<InstallKind | null>(null);

  /** "found", "none", or "failed" when GitHub couldn't be reached. */
  const lookForUpdate = useCallback(async (): Promise<UpdateCheck> => {
    try {
      const update = await check();
      if (update) setState((cur) => (cur.kind === "installing" ? cur : { kind: "available", update }));
      return update ? "found" : "none";
    } catch {
      // Offline, rate-limited or running outside the installed app: try again later.
      return "failed";
    }
  }, []);

  useEffect(() => {
    api.installKind().then(setInstallKind, () => setInstallKind("portable"));
  }, []);

  useEffect(() => {
    if (installKind !== "installer") return;
    void lookForUpdate();
    const timer = window.setInterval(() => void lookForUpdate(), APP_UPDATE_CHECK_MS);
    return () => window.clearInterval(timer);
  }, [installKind, lookForUpdate]);

  const install = useCallback(async (update: Update) => {
    let total = 0;
    let done = 0;
    setState({ kind: "installing", version: update.version, percent: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null;
          setState({ kind: "installing", version: update.version, percent });
        }
      });
      // On Windows the installer usually closes the app itself; relaunch covers the rest.
      await relaunch();
    } catch (e) {
      setState({ kind: "failed", version: update.version, error: errorText(e) });
    }
  }, []);

  const dismiss = useCallback(() => setState({ kind: "idle" }), []);

  return { state, install, dismiss, lookForUpdate, installKind };
}

interface UpdateBarProps {
  state: AppUpdateState;
  onInstall: (update: Update) => void;
  onDismiss: () => void;
}

/** A thin bar under the top bar. Shown only when a new version exists. */
export function UpdateBar({ state, onInstall, onDismiss }: UpdateBarProps) {
  if (state.kind === "idle") return null;
  return (
    <div className={`update-bar ${state.kind === "failed" ? "is-bad" : ""}`} role="status">
      {state.kind === "available" && (
        <>
          <p>
            Version {state.update.version} of Deadlock Config Manager is available.{" "}
            <span className="muted">You're on {state.update.currentVersion}.</span>
          </p>
          <span className="spacer" />
          <button className="btn btn-quiet btn-sm" onClick={onDismiss}>
            Later
          </button>
          <button className="btn btn-secondary btn-sm" onClick={() => onInstall(state.update)}>
            Install and restart
          </button>
        </>
      )}
      {state.kind === "installing" && (
        <p>
          Downloading version {state.version}
          {state.percent !== null ? ` (${state.percent}%)` : "…"} The app restarts when it's done.
        </p>
      )}
      {state.kind === "failed" && (
        <>
          <p>
            Couldn't install version {state.version}: {state.error}
          </p>
          <span className="spacer" />
          <button className="btn btn-quiet btn-sm" onClick={onDismiss}>
            Dismiss
          </button>
        </>
      )}
    </div>
  );
}
