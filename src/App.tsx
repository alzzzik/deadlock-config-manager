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
import { StatusStrip } from "./components/StatusStrip";
import { PinnedSection, RepoSection } from "./components/RepoSection";
import { FolderIcon, RefreshIcon, SearchIcon } from "./components/icons";
import { AddRepo } from "./components/AddRepo";
import { RestoreDialog } from "./components/RestoreDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { PreviewDialog } from "./components/PreviewDialog";
import { ToastStack, useToasts } from "./components/Toasts";
import { UpdateBar, useAppUpdate } from "./components/AppUpdate";

type Dialog =
  | { kind: "none" }
  | { kind: "restore" }
  | { kind: "settings" }
  | { kind: "preview"; preview: Preview; proceed: () => void };

type ApplyCall = (confirmed: boolean) => Promise<ApplyResponse>;

/** How often linked repos are re-checked while the app is open. */
const UPDATE_CHECK_MS = 60 * 60 * 1000;
/** Longest the startup splash stays up while waiting for data. */
const SPLASH_MAX_MS = 2500;
/** Matches the fade duration in index.html. */
const SPLASH_FADE_MS = 250;
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
  const seenFirstLoad = useRef(false);
  /** True once packs have come from GitHub this session (not just the disk cache). */
  const [fresh, setFresh] = useState(false);
  const [ready, setReady] = useState(false);
  const { toasts, push, dismiss } = useToasts();
  const appUpdate = useAppUpdate();
  const fail = useCallback((e: unknown) => push("error", errorText(e)), [push]);

  const refreshOverview = useCallback(() => {
    api.overview().then(setOverview, fail);
  }, [fail]);

  const refreshPacks = useCallback((silent = false) => {
    if (!silent) setLoading(true);
    api
      .listPacks()
      // Offline repos are flagged in their own header, so no toast here.
      .then((list) => {
        setGroups(list);
        setFresh(true);
      })
      .catch((e) => !silent && fail(e))
      .finally(() => setLoading(false));
  }, [fail]);

  useEffect(() => {
    refreshOverview();
    // Show last session's packs immediately, then refresh them from GitHub.
    api
      .cachedPacks()
      .then((cached) => cached.length > 0 && setGroups((cur) => cur ?? cached))
      .catch(() => undefined);
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

  // Fade the splash out once status and some packs are in, or after SPLASH_MAX_MS regardless.
  useEffect(() => {
    if (!ready && overview && groups) setReady(true);
  }, [ready, overview, groups]);
  useEffect(() => {
    const timer = window.setTimeout(() => setReady(true), SPLASH_MAX_MS);
    return () => window.clearTimeout(timer);
  }, []);
  useEffect(() => {
    if (!ready) return;
    document.body.classList.add("app-ready");
    const timer = window.setTimeout(() => document.getElementById("splash")?.remove(), SPLASH_FADE_MS);
    return () => window.clearTimeout(timer);
  }, [ready]);

  const firstRepo = overview?.repos[0];
  const appliedPack = useMemo(
    () => groups?.flatMap((g) => g.packs).find((p) => isSamePack(overview?.applied ?? null, p, firstRepo)),
    [groups, overview, firstRepo],
  );
  const updateAvailable = !!appliedPack?.revision && !!overview?.applied?.revision && appliedPack.revision !== overview.applied.revision;

  // Toast once when a background check finds a new revision of the applied pack. On startup the
  // status strip already says "Update available", so the first load is recorded silently.
  useEffect(() => {
    if (!fresh || !groups || !overview) return;
    const rev = appliedPack?.revision ?? null;
    if (!seenFirstLoad.current) {
      seenFirstLoad.current = true;
      notifiedRevision.current = rev;
      return;
    }
    if (updateAvailable && appliedPack && rev && notifiedRevision.current !== rev) {
      notifiedRevision.current = rev;
      const what = appliedPack.commitMessage ? `: "${appliedPack.commitMessage}"` : "";
      push("info", `${appliedPack.name} has an update${what}. Re-apply to get it.`);
    }
  }, [fresh, groups, overview, updateAvailable, appliedPack, push]);

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

  const packCount = allPacks.length;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark" aria-hidden />
          Deadlock Config Manager
        </div>
        <span className="spacer" />
        {busy && <span className="working">Working…</span>}
        {overview && (
          <span className={`game-state ${overview.deadlockRunning ? "is-running" : ""}`} title="Files can only be changed while Deadlock is closed">
            <span className="game-dot" aria-hidden />
            {overview.deadlockRunning ? "Deadlock running" : "Deadlock closed"}
          </span>
        )}
        <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "restore" })}>
          Restore…
        </button>
        <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "settings" })} disabled={!overview}>
          Settings
        </button>
      </header>

      <UpdateBar state={appUpdate.state} onInstall={(u) => void appUpdate.install(u)} onDismiss={appUpdate.dismiss} />

      <main className="content">
        {overview ? (
          <StatusStrip
            overview={overview}
            busy={busy}
            updateAvailable={updateAvailable}
            updateNote={updateAvailable ? appliedPack?.commitMessage ?? null : null}
            canReapply={!!appliedPack || overview.applied?.packId === "local"}
            onReapply={reapply}
            onAccount={(id) => api.setAccount(id).then(setOverview, fail)}
            onLocate={locate}
            onAuto={auto}
            onOpen={openLocation}
          />
        ) : (
          <section className="status is-loading" aria-busy="true" />
        )}

        <section className="packs" aria-labelledby="packs-title">
          <div className="toolbar">
            <h2 id="packs-title">
              Packs {packCount > 0 && <span className="count">{packCount}</span>}
            </h2>
            <span className="spacer" />
            {packCount > SEARCH_MIN_PACKS && (
              <label className="search">
                <SearchIcon size={14} />
                <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search packs" aria-label="Search packs" />
              </label>
            )}
            <button className="icon-btn" onClick={() => refreshPacks()} disabled={loading} aria-label="Refresh all repos" title="Refresh all repos">
              <RefreshIcon className={loading ? "spin" : ""} />
            </button>
            <button className="btn btn-quiet btn-sm" onClick={applyLocal} disabled={blocked} title="Apply a pack from a folder on this PC">
              <FolderIcon size={14} />
              From folder…
            </button>
          </div>

          {pinnedPacks.length > 0 && (
            <PinnedSection
              packs={pinnedPacks}
              pinned={overview?.pinned ?? []}
              firstRepo={firstRepo}
              applied={overview?.applied ?? null}
              disabled={blocked}
              onApply={applyPack}
              onOpenGithub={openGithub}
              onTogglePin={togglePin}
            />
          )}

          {query.trim() && filtered.length === 0 && pinnedPacks.length === 0 && <p className="empty">No packs match “{query.trim()}”.</p>}

          {!groups && loading && (
            <ul className="pack-list" aria-busy="true">
              {[0, 1, 2].map((i) => (
                <li key={i} className="pack-row is-loading" />
              ))}
            </ul>
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

          <AddRepo linked={overview?.repos ?? []} onAdd={addRepo} onOpenGithub={openGithub} />
        </section>
      </main>

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
          onOpenGithub={openGithub}
        />
      )}
      {dialog.kind === "preview" && (
        <PreviewDialog preview={dialog.preview} onCancel={() => setDialog({ kind: "none" })} onConfirm={dialog.proceed} />
      )}

      <ToastStack toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}
