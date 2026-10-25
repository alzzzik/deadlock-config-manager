import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrent as getDeepLinks, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import {
  ApplyResponse,
  BackupMeta,
  LIBRARY_PREFIX,
  LaunchFailure,
  LibraryEntry,
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
import { FirstRunGuide, GuideChoice } from "./components/FirstRunGuide";
import { QuickSettings } from "./components/QuickSettings";
import { MyConfigs, SaveConfigDialog } from "./components/MyConfigs";
import { ImportDialog } from "./components/ImportDialog";
import { StoreDialog } from "./components/StoreDialog";
import { LaunchIssue } from "./components/LaunchIssue";
import { PerfDialog } from "./components/PerfDialog";

type Dialog =
  | { kind: "none" }
  | { kind: "restore" }
  | { kind: "settings" }
  | { kind: "guide" }
  | { kind: "saveConfig" }
  | { kind: "import" }
  | { kind: "store" }
  | { kind: "perf" }
  | { kind: "preview"; preview: Preview; proceed: () => void; proceedSafe: () => void };

type ApplyCall = (confirmed: boolean, rankedSafe: boolean) => Promise<ApplyResponse>;

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
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const [launchIssue, setLaunchIssue] = useState<LaunchFailure | null>(null);
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
    api.listLibrary().then(setLibrary, () => undefined);
  }, []);

  useEffect(() => {
    refreshOverview();
    // Show last session's packs immediately, then refresh them from GitHub.
    api
      .cachedPacks()
      .then((cached) => cached.length > 0 && setGroups((cur) => cur ?? cached))
      .catch(() => undefined);
    refreshPacks();
    // Re-check gameinfo.gi whenever the window regains focus (e.g. after a Steam update),
    // and when the background watcher re-applies after a game update.
    window.addEventListener("focus", refreshOverview);
    let unlistenFiles: (() => void) | undefined;
    listen("files-changed", refreshOverview).then((u) => (unlistenFiles = u), () => undefined);
    const loadLaunchIssue = () => api.launchIssue().then(setLaunchIssue, () => undefined);
    loadLaunchIssue();
    let unlistenLaunch: (() => void) | undefined;
    listen("launch-failed", loadLaunchIssue).then((u) => (unlistenLaunch = u), () => undefined);
    // Background check for pack updates while the app stays open.
    const timer = window.setInterval(() => refreshPacks(true), UPDATE_CHECK_MS);
    return () => {
      window.removeEventListener("focus", refreshOverview);
      unlistenFiles?.();
      unlistenLaunch?.();
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

  // First launch: offer the guide once the window is ready.
  const guideShown = useRef(false);
  useEffect(() => {
    if (!ready || guideShown.current || !overview?.showGuide || dialog.kind !== "none") return;
    guideShown.current = true;
    setDialog({ kind: "guide" });
  }, [ready, overview, dialog.kind]);

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

  const runApply = async (call: ApplyCall, confirmed = false, rankedSafe = false) => {
    setBusy(true);
    try {
      const res = await call(confirmed, rankedSafe);
      if (res.kind === "preview") {
        setDialog({
          kind: "preview",
          preview: res.preview,
          proceed: () => {
            setDialog({ kind: "none" });
            void runApply(call, true, rankedSafe);
          },
          proceedSafe: () => {
            setDialog({ kind: "none" });
            void runApply(call, true, true);
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

  const applyPack = (p: PackInfo, rankedSafe = false) => runApply((c, r) => api.applyPack(p, c, r), false, rankedSafe);

  const applyLocal = async (rankedSafe = false) => {
    const folder = await open({ directory: true, title: "Folder with gameinfo.gi and video.txt" });
    if (typeof folder === "string") void runApply((c, r) => api.applyLocal(folder, c, r), false, rankedSafe);
  };

  const makeRankedSafe = () => runApply((c) => api.makeRankedSafe(c));

  const applyLibrary = (e: LibraryEntry, rankedSafe = false) =>
    runApply((c, r) => api.applyLibrary(e.id, c, r), false, rankedSafe);

  const sharePack = (p: PackInfo) => void copyShare(api.sharePack(p, false));

  const copyShare = async (code: Promise<string>) => {
    try {
      const c = await code;
      await navigator.clipboard.writeText(c);
      push("ok", `Share code copied: ${c}`);
    } catch (e) {
      fail(e);
    }
  };

  const addedToLibrary = (e: LibraryEntry, verb: string) => {
    setLibrary((cur) => [e, ...cur.filter((x) => x.id !== e.id)]);
    push("ok", `${verb} "${e.name}" to My configs.`);
  };

  const importCode = async (text: string) => {
    setBusy(true);
    try {
      const isBanana = !/DCM-/i.test(text) && /gamebanana\.com\/mods\/\d+/i.test(text);
      addedToLibrary(await (isBanana ? api.importGamebanana(text.trim()) : api.importCode(text)), "Added");
      setDialog({ kind: "none" });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  /** From Browse: adds a GameBanana config and leaves the dialog open. Resolves to an error message or null. */
  const addFromGamebanana = async (url: string): Promise<string | null> => {
    try {
      addedToLibrary(await api.importGamebanana(url), "Added");
      return null;
    } catch (e) {
      return errorText(e);
    }
  };

  const importPath = async (directory: boolean) => {
    const path = await open(
      directory
        ? { directory: true, title: "Folder with gameinfo.gi" }
        : { title: "Config zip", filters: [{ name: "Zip", extensions: ["zip"] }] },
    );
    if (typeof path !== "string") return;
    try {
      addedToLibrary(await api.importConfig(path), "Added");
      setDialog({ kind: "none" });
    } catch (e) {
      fail(e);
    }
  };

  const exportConfig = async (e: LibraryEntry) => {
    const path = await save({ title: "Export config", defaultPath: `${e.name}.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (!path) return;
    api.exportConfig(e.id, path).then(() => push("ok", `Exported ${e.name}.`), fail);
  };

  const deleteConfig = (e: LibraryEntry) =>
    api.deleteConfig(e.id).then((list) => {
      setLibrary(list);
      push("info", `Deleted ${e.name}.`);
    }, fail);

  const saveCurrent = (name: string, notes: string) =>
    api.saveCurrent(name, notes).then((e) => {
      setDialog({ kind: "none" });
      addedToLibrary(e, "Saved");
    }, fail);

  // deadlockcfg://import/<code> links: on launch and while running.
  const importCodeRef = useRef(importCode);
  importCodeRef.current = importCode;
  useEffect(() => {
    // Links can come from any web page, so ask before adding anything.
    const handle = (urls: string[] | null) =>
      urls
        ?.filter((u) => u.toUpperCase().includes("DCM-"))
        .slice(0, 1)
        .forEach((u) => {
          if (window.confirm("A deadlockcfg:// link wants to add a shared config to My configs. Nothing is applied until you choose to. Add it?")) {
            void importCodeRef.current(u);
          }
        });
    getDeepLinks().then(handle, () => undefined);
    let unlisten: (() => void) | undefined;
    onOpenUrl(handle).then((u) => (unlisten = u), () => undefined);
    return () => unlisten?.();
  }, []);

  const skipGuide = () => {
    setDialog({ kind: "none" });
    api.finishGuide().then(setOverview, fail);
  };

  /** Links the choice's repo if needed, then opens the ranked-safe preview of that pack. */
  const pickFromGuide = async (choice: GuideChoice) => {
    setBusy(true);
    try {
      const linked = overview?.repos.some((r) => r.toLowerCase() === choice.repo.toLowerCase());
      let list = groups ?? (await api.listPacks());
      if (!linked) {
        const { overview: o, group } = await api.addRepo(choice.repo);
        setOverview(o);
        list = [...list, group];
        setGroups(list);
      }
      const pack = list
        .filter((g) => g.repo.toLowerCase() === choice.repo.toLowerCase())
        .flatMap((g) => g.packs)
        .find((p) => p.folder.toLowerCase() === choice.folder.toLowerCase());
      if (!pack) throw new Error(`${choice.title} wasn't found in ${choice.repo}. Pick a pack from the list instead.`);
      setDialog({ kind: "none" });
      setBusy(false);
      await applyPack(pack);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const reapply = () => {
    const safe = !!overview?.applied?.rankedSafe;
    const appliedId = overview?.applied?.packId ?? "";
    const saved = library.find((e) => `${LIBRARY_PREFIX}${e.id}` === appliedId);
    if (saved) return applyLibrary(saved, safe);
    if (appliedPack) return applyPack(appliedPack, safe);
    if (overview?.applied?.packId === "local") return applyLocal(safe);
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
      setGroups((cur) => [...(cur ?? []).filter((g) => g.repo.toLowerCase() !== group.repo.toLowerCase()), group]);
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

  const closeLaunchIssue = () => api.dismissLaunchIssue().then(() => setLaunchIssue(null), fail);

  /** "Restore previous files" from the crash prompt: the backup taken right before the last write. */
  const restoreAfterFailedLaunch = async (backupId: string) => {
    setBusy(true);
    try {
      push("ok", await api.restore(backupId));
      await closeLaunchIssue();
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
  const isLibraryApplied = library.some((e) => `${LIBRARY_PREFIX}${e.id}` === overview?.applied?.packId);
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
        <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "perf" })} disabled={!overview}>
          Performance
        </button>
        <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "restore" })}>
          Restore…
        </button>
        <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "settings" })} disabled={!overview}>
          Settings
        </button>
      </header>

      <UpdateBar state={appUpdate.state} onInstall={(u) => void appUpdate.install(u)} onDismiss={appUpdate.dismiss} />

      <main className="content">
        {launchIssue && (
          <LaunchIssue
            issue={launchIssue}
            busy={busy || !!overview?.deadlockRunning}
            onRestore={() => void restoreAfterFailedLaunch(launchIssue.backupId)}
            onKeep={() => void closeLaunchIssue()}
          />
        )}
        {overview ? (
          <StatusStrip
            overview={overview}
            busy={busy}
            updateAvailable={updateAvailable}
            updateNote={updateAvailable ? appliedPack?.commitMessage ?? null : null}
            canReapply={!!appliedPack || overview.applied?.packId === "local" || isLibraryApplied}
            onReapply={reapply}
            onMakeRankedSafe={makeRankedSafe}
            onAccount={(id) => api.setAccount(id).then(setOverview, fail)}
            onLocate={locate}
            onAuto={auto}
            onOpen={openLocation}
          />
        ) : (
          <section className="status is-loading" aria-busy="true" />
        )}

        {overview?.gameinfoFound && (
          <QuickSettings
            refreshKey={`${overview.applied?.appliedAt ?? ""}|${overview.status.kind}`}
            disabled={busy || overview.deadlockRunning}
            onApplied={(m) => {
              push("ok", m);
              refreshOverview();
            }}
            onError={fail}
          />
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
            <button className="btn btn-quiet btn-sm" onClick={() => setDialog({ kind: "store" })} title="Browse community configs">
              Browse…
            </button>
            <button className="btn btn-quiet btn-sm" onClick={() => void applyLocal()} disabled={blocked} title="Apply a pack from a folder on this PC">
              <FolderIcon size={14} />
              From folder…
            </button>
          </div>

          <MyConfigs
            entries={library}
            appliedId={overview?.applied?.packId ?? null}
            disabled={blocked}
            canSave={!!overview?.gameinfoFound}
            onApply={(e) => void applyLibrary(e)}
            onSave={() => setDialog({ kind: "saveConfig" })}
            onImport={() => setDialog({ kind: "import" })}
            onExport={(e) => void exportConfig(e)}
            onDelete={(e) => void deleteConfig(e)}
          />

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
              onShare={sharePack}
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
              onShare={sharePack}
            />
          ))}

          <AddRepo linked={overview?.repos ?? []} onAdd={addRepo} onOpenGithub={openGithub} onBrowse={() => setDialog({ kind: "store" })} />
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
          onBrowse={() => setDialog({ kind: "store" })}
          onMoveRepo={moveRepo}
          onRemoveRepo={removeRepo}
          onOpenBackups={() => openLocation("backups")}
          onOpenGithub={openGithub}
          onCheckUpdate={appUpdate.lookForUpdate}
          onShowGuide={() => setDialog({ kind: "guide" })}
          installKind={appUpdate.installKind}
          onNotify={push}
        />
      )}
      {dialog.kind === "saveConfig" && (
        <SaveConfigDialog
          suggestedName={overview?.applied ? `My ${overview.applied.packName}` : "My config"}
          onCancel={() => setDialog({ kind: "none" })}
          onSave={(n, notes) => void saveCurrent(n, notes)}
        />
      )}
      {dialog.kind === "import" && (
        <ImportDialog
          busy={busy}
          onCancel={() => setDialog({ kind: "none" })}
          onCode={(t) => void importCode(t)}
          onZip={() => void importPath(false)}
          onFolder={() => void importPath(true)}
        />
      )}
      {dialog.kind === "store" && (
        <StoreDialog
          linked={overview?.repos ?? []}
          busy={busy}
          onClose={() => setDialog({ kind: "none" })}
          onLinkRepo={addRepo}
          onAddCode={(code) => void importCode(code)}
          onAddGamebanana={addFromGamebanana}
          onOpenGithub={openGithub}
        />
      )}
      {dialog.kind === "perf" && (
        <PerfDialog onClose={() => setDialog({ kind: "none" })} onNotify={push} />
      )}
      {dialog.kind === "guide" && <FirstRunGuide busy={busy} onPick={(c) => void pickFromGuide(c)} onSkip={skipGuide} />}
      {dialog.kind === "preview" && (
        <PreviewDialog preview={dialog.preview} onCancel={() => setDialog({ kind: "none" })} onConfirm={dialog.proceed} onConfirmSafe={dialog.proceedSafe} />
      )}

      <ToastStack toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}
