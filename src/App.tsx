import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ApplyResponse,
  BackupMeta,
  Location,
  Overview,
  PackInfo,
  PathKind,
  Preview,
  RepoGroup,
  api,
  errorText,
  isSamePack,
  matchesSearch,
  pinKey,
} from "./api";
import { FilesPanel } from "./components/FilesPanel";
import { RepoSection } from "./components/RepoSection";
import { PackCard } from "./components/PackCard";
import { AddRepo } from "./components/AddRepo";
import { RestoreDialog } from "./components/RestoreDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { PreviewDialog } from "./components/PreviewDialog";
import { ToastStack, useToasts } from "./components/Toasts";

type Dialog =
  | { kind: "none" }
  | { kind: "restore" }
  | { kind: "settings" }
  | { kind: "preview"; preview: Preview; proceed: () => void };

type ApplyCall = (confirmed: boolean) => Promise<ApplyResponse>;

/** How often linked repos are re-checked while the app is open. */
const UPDATE_CHECK_MS = 60 * 60 * 1000;
/** The search box appears once there are more packs than this. */
const SEARCH_MIN_PACKS = 3;

const PICKER_TITLES: Record<PathKind, string> = {
  steam: "Steam install folder",
  gameinfo: "Deadlock's game\\citadel folder (or the Deadlock folder)",
  video: "Folder with video.txt (userdata\\<id>\\1422450\\local\\cfg)",
};

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [groups, setGroups] = useState<RepoGroup[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [dialog, setDialog] = useState<Dialog>({ kind: "none" });
  const [query, setQuery] = useState("");
  const notifiedRevision = useRef<string | null>(null);
  const { toasts, push, dismiss } = useToasts();
  const fail = useCallback((e: unknown) => push("error", errorText(e)), [push]);

  const refreshOverview = useCallback(() => {
    api.overview().then(setOverview, fail);
  }, [fail]);

  const refreshPacks = useCallback((silent = false) => {
    if (!silent) setLoading(true);
    api
      .listPacks()
      // Offline repos are flagged in their own header, so no toast here.
      .then(setGroups)
      .catch((e) => !silent && fail(e))
      .finally(() => setLoading(false));
  }, [fail]);

  useEffect(() => {
    refreshOverview();
    refreshPacks();
    // Re-check gameinfo.gi whenever the window regains focus (e.g. after a Steam update).
    window.addEventListener("focus", refreshOverview);
    // Background check for pack updates while the app stays open.
    const timer = window.setInterval(() => refreshPacks(true), UPDATE_CHECK_MS);
    return () => {
      window.removeEventListener("focus", refreshOverview);
      window.clearInterval(timer);
    };
  }, [refreshOverview, refreshPacks]);

  const firstRepo = overview?.repos[0];
  const appliedPack = useMemo(
    () => groups?.flatMap((g) => g.packs).find((p) => isSamePack(overview?.applied ?? null, p, firstRepo)),
    [groups, overview, firstRepo],
  );
  const updateAvailable = !!appliedPack?.revision && !!overview?.applied?.revision && appliedPack.revision !== overview.applied.revision;

  // Toast once per new revision of the applied pack.
  useEffect(() => {
    const rev = appliedPack?.revision ?? null;
    if (updateAvailable && appliedPack && rev && notifiedRevision.current !== rev) {
      notifiedRevision.current = rev;
      const what = appliedPack.commitMessage ? `: "${appliedPack.commitMessage}"` : "";
      push("info", `${appliedPack.name} has an update${what}. Re-apply to get it.`);
    }
  }, [updateAvailable, appliedPack, push]);

  const allPacks = useMemo(() => groups?.flatMap((g) => g.packs) ?? [], [groups]);
  const pinnedPacks = useMemo(
    () =>
      (overview?.pinned ?? [])
        .map((k) => allPacks.find((p) => pinKey(p) === k))
        .filter((p): p is PackInfo => !!p && matchesSearch(p, query)),
    [overview, allPacks, query],
  );
  const filtered = useMemo(
    () =>
      (groups ?? [])
        .map((g) => ({ group: g, packs: g.packs.filter((p) => matchesSearch(p, query)) }))
        .filter((x) => !query.trim() || x.packs.length > 0),
    [groups, query],
  );

  const runApply = async (call: ApplyCall, confirmed = false) => {
    setBusy(true);
    try {
      const res = await call(confirmed);
      if (res.kind === "preview") {
        setDialog({
          kind: "preview",
          preview: res.preview,
          proceed: () => {
            setDialog({ kind: "none" });
            void runApply(call, true);
          },
        });
        return;
      }
      push("ok", res.message);
      res.notes.forEach((n) => push("info", n));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      refreshOverview();
    }
  };

  const applyPack = (p: PackInfo) => runApply((c) => api.applyPack(p, c));

  const applyLocal = async () => {
    const folder = await open({ directory: true, title: "Folder with gameinfo.gi and video.txt" });
    if (typeof folder === "string") void runApply((c) => api.applyLocal(folder, c));
  };

  const reapply = () => {
    if (appliedPack) return applyPack(appliedPack);
    if (overview?.applied?.packId === "local") return applyLocal();
    push("error", "That pack isn't in your linked repos anymore. Refresh, or pick a pack below.");
  };

  const locate = async (kind: PathKind) => {
    // Start where the current file lives instead of Windows' default (Documents).
    const current = kind === "steam" ? overview?.steamRoot : kind === "gameinfo" ? overview?.gameinfoPath : overview?.videoPath;
    const defaultPath = (kind === "steam" ? current : current?.replace(/[\\/][^\\/]*$/, "")) ?? overview?.steamRoot ?? undefined;
    const picked = await open({ directory: true, title: PICKER_TITLES[kind], defaultPath });
    if (typeof picked === "string") api.setPath(kind, picked).then(setOverview, fail);
  };

  const auto = (kind: PathKind) =>
    api.setPath(kind, null).then((o) => {
      setOverview(o);
      push("info", o.gameinfoFound ? "Detected automatically." : "Deadlock wasn't found automatically. Use Locate… to pick it.");
    }, fail);

  const openLocation = (which: Location) => api.open(which).catch(fail);
  const openGithub = (url: string) => api.openGithub(url).catch(fail);
  const togglePin = (p: PackInfo) => api.togglePin(pinKey(p)).then(setOverview, fail);

  const addRepo = async (input: string): Promise<string | null> => {
    try {
      const { overview: o, group } = await api.addRepo(input);
      setOverview(o);
      setGroups((cur) => [...(cur ?? []), group]);
      push("ok", `Linked ${group.repo} (${group.packs.length} pack${group.packs.length === 1 ? "" : "s"}).`);
      return null;
    } catch (e) {
      return errorText(e);
    }
  };

  const reorder = (o: Overview) => {
    setOverview(o);
    setGroups((cur) => cur && o.repos.map((r) => cur.find((g) => g.repo === r)).filter((g): g is RepoGroup => !!g));
  };
  const moveRepo = (repo: string, delta: number) => api.moveRepo(repo, delta).then(reorder, fail);
  const removeRepo = (repo: string) =>
    api.removeRepo(repo).then((o) => {
      reorder(o);
      push("info", `Removed ${repo}.`);
      if (!groups?.some((g) => o.repos.includes(g.repo))) refreshPacks();
    }, fail);

  const restore = async (b: BackupMeta) => {
    setBusy(true);
    try {
      push("ok", await api.restore(b.id));
      setDialog({ kind: "none" });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      refreshOverview();
    }
  };

  const saveKeys = (keys: string[]) =>
    api.saveSettings(keys).then((o) => {
      setOverview(o);
      setDialog({ kind: "none" });
      push("ok", "Settings saved");
    }, fail);

  const canWrite = !!overview && (overview.gameinfoFound || overview.gameinfoCreatable);
  const blocked = busy || !overview || overview.deadlockRunning || !canWrite;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark" aria-hidden />
          <span>Deadlock Config Manager</span>
        </div>
        {busy && <span className="working">Working…</span>}
      </header>

      <main className="content">
        {overview ? (
          <FilesPanel
            overview={overview}
            busy={busy}
            updateAvailable={updateAvailable}
            canReapply={!!appliedPack || overview.applied?.packId === "local"}
            onReapply={reapply}
            onAccount={(id) => api.setAccount(id).then(setOverview, fail)}
            onLocate={locate}
            onAuto={auto}
            onOpen={openLocation}
          />
        ) : (
          <section className="files-panel skeleton" />
        )}

        <div className="packs">
          <div className="section-head">
            <div>
              <p className="eyebrow">Config packs</p>
              <h2>From your linked repos</h2>
            </div>
            <div className="section-tools">
              {allPacks.length > SEARCH_MIN_PACKS && (
                <input
                  className="search"
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search packs"
                  aria-label="Search packs"
                />
              )}
              <button className="btn btn-ghost" onClick={() => refreshPacks()} disabled={loading}>
                {loading ? "Refreshing…" : "Refresh all"}
              </button>
            </div>
          </div>

          {pinnedPacks.length > 0 && (
            <section className="repo pinned" aria-label="Pinned packs">
              <header className="repo-head">
                <span className="pin-mark" aria-hidden>
                  ★
                </span>
                <span className="repo-title">Pinned</span>
              </header>
              <div className="pack-grid">
                {pinnedPacks.map((p, i) => {
                  const isApplied = isSamePack(overview?.applied ?? null, p, firstRepo);
                  return (
                    <PackCard
                      key={pinKey(p)}
                      pack={p}
                      index={i}
                      applied={isApplied}
                      hasUpdate={isApplied && updateAvailable}
                      disabled={blocked}
                      pinned
                      showRepo
                      onApply={applyPack}
                      onOpenGithub={openGithub}
                      onTogglePin={togglePin}
                    />
                  );
                })}
              </div>
            </section>
          )}

          {query.trim() && filtered.length === 0 && pinnedPacks.length === 0 && (
            <p className="empty">No packs match “{query.trim()}”.</p>
          )}

          {!groups && loading && (
            <div className="pack-grid">
              {[0, 1].map((i) => (
                <div key={i} className="pack-card skeleton" />
              ))}
            </div>
          )}

          {filtered.map(({ group: g, packs }, i) => (
            <RepoSection
              key={g.repo}
              group={g}
              packs={packs}
              pinned={overview?.pinned ?? []}
              index={i}
              count={filtered.length}
              firstRepo={firstRepo}
              applied={overview?.applied ?? null}
              disabled={blocked}
              loading={loading}
              onApply={applyPack}
              onRefresh={() => refreshPacks()}
              onOpenGithub={openGithub}
              onMove={moveRepo}
              onRemove={removeRepo}
              onTogglePin={togglePin}
            />
          ))}

          <AddRepo onAdd={addRepo} />
        </div>
      </main>

      <footer className="bottombar">
        <button className="btn" onClick={() => setDialog({ kind: "restore" })}>
          Restore…
        </button>
        <button className="btn btn-ghost" onClick={applyLocal} disabled={blocked}>
          Apply from folder…
        </button>
        <span className="spacer" />
        {overview && (
          <span className={`game-state ${overview.deadlockRunning ? "is-running" : ""}`}>
            <span className="dot" aria-hidden />
            {overview.deadlockRunning ? "Deadlock running" : "Deadlock closed"}
          </span>
        )}
        <button className="btn btn-ghost" onClick={() => setDialog({ kind: "settings" })} disabled={!overview}>
          Settings
        </button>
      </footer>

      {dialog.kind === "restore" && (
        <RestoreDialog busy={busy} onClose={() => setDialog({ kind: "none" })} onRestore={restore} onOpenFolder={() => openLocation("backups")} />
      )}
      {dialog.kind === "settings" && overview && (
        <SettingsDialog
          overview={overview}
          onClose={() => setDialog({ kind: "none" })}
          onSaveKeys={saveKeys}
          onAddRepo={addRepo}
          onMoveRepo={moveRepo}
          onRemoveRepo={removeRepo}
          onOpenBackups={() => openLocation("backups")}
        />
      )}
      {dialog.kind === "preview" && (
        <PreviewDialog preview={dialog.preview} onCancel={() => setDialog({ kind: "none" })} onConfirm={dialog.proceed} />
      )}

      <ToastStack toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}
