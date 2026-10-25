import { invoke } from "@tauri-apps/api/core";

export type Account = { id: string; persona: string | null; videoPath: string; hasVideo: boolean };

export type Status =
  | { kind: "notApplied" }
  | { kind: "missing" }
  | { kind: "applied"; pack: string }
  | { kind: "appliedDmm"; pack: string }
  | { kind: "changed"; pack: string };

export type AppliedRecord = {
  repo: string | null;
  packId: string;
  packName: string;
  revision: string | null;
  appliedAt: string;
  rankedSafe?: boolean;
};

export type Overview = {
  steamRoot: string | null;
  gameinfoPath: string | null;
  gameinfoFound: boolean;
  gameinfoCreatable: boolean;
  videoPath: string | null;
  videoFound: boolean;
  accounts: Account[];
  accountId: string | null;
  status: Status;
  applied: AppliedRecord | null;
  deadlockRunning: boolean;
  repos: string[];
  pinned: string[];
  preservedKeys: string[];
  optionalPreserved: string[];
  alwaysPreserved: string[];
  /** Engine-section entries in the installed gameinfo.gi that differ from Valve's (can block ranked). */
  rankedChanges: number | null;
  /** console.log shows Deadlock refused ranked matchmaking because of gameinfo.gi. */
  rankedBlocked: boolean;
  showGuide: boolean;
  changedBy: "game" | "dmm" | "grimoire" | "other" | null;
  gameBuild: string | null;
};

export type PackInfo = {
  repo: string;
  id: string;
  name: string;
  description: string;
  folder: string;
  lastCommitDate: string | null;
  revision: string | null;
  commitMessage: string | null;
  htmlUrl: string;
  videoFile?: string | null;
  gameinfoFile?: string | null;
};

export type RepoGroup = {
  repo: string;
  url: string;
  updatedAt: string | null;
  packs: PackInfo[];
  offline: boolean;
  error: string | null;
};

export type Change = { key: string; before: string | null; after: string | null };
export type FileDiff = { file: string; created: boolean; changes: Change[]; unchanged: number };
export type Preview = {
  packName: string;
  files: FileDiff[];
  warnings: string[];
  /** Engine-section entries that differ from Valve's file after applying. */
  rankedChanges: Change[];
  rankedSafe: boolean;
  /** Keyed by change path ("ConVars › r_ssao"). */
  convars: Record<string, ConvarInfo>;
  convarBuild: string;
  /** Personal tweaks added on top of the pack. */
  tweaks: number;
  /** Overlay mode: settings and sections added from Valve's current file; null when overlay is off. */
  overlayAdded: number | null;
};

export type UpdateReport = {
  fromBuild: string | null;
  toBuild: string | null;
  newSettings: string[];
  ignored: string[];
  engineChanges: number;
};

export type StoreEntry =
  | {
      kind: "repo";
      repo: string;
      title: string;
      description: string;
      author: string;
      license: string;
      updated: string | null;
      configs: string[];
    }
  | { kind: "gamebanana"; id: number; title: string; description: string; author: string; updated: string | null }
  | { kind: "code"; title: string; description: string; author: string; code: string };
export type StoreView = { entries: StoreEntry[]; checked: string | null; online: boolean };

export type PerfReport = {
  time: string;
  map: string;
  frames: number;
  seconds: number;
  avgFps: number;
  p1Fps: number;
  frameMs: number;
  frameMsP99: number;
};
export type PerfSession = PerfReport & { configKey: string; configName: string; rankedSafe: boolean; gameBuild: string | null; recordedAt: string };
export type ConfigStats = {
  configKey: string;
  configName: string;
  sessions: number;
  minutes: number;
  medianAvgFps: number;
  medianP1Fps: number;
  medianFrameMsP99: number;
  lastPlayed: string;
};
export type PerfView = { stats: ConfigStats[]; recent: PerfSession[]; logging: boolean; minSeconds: number; minSessions: number };

export type GithubStatus = { available: boolean; login: string | null };
export type DeviceCode = { deviceCode: string; userCode: string; verificationUri: string; interval: number; expiresIn: number };
export type PollResult = { state: "pending"; interval: number } | { state: "done"; login: string } | { state: "failed"; message: string };

export type Automation = { overlay: boolean; autoReapply: boolean; startWithWindows: boolean };

export type ConvarInfo = { text: string | null; ignored: boolean; default: string | null };

export type LibraryEntry = {
  id: string;
  name: string;
  notes: string;
  createdAt: string;
  hasVideo: boolean;
  source: string;
};

/** One editable setting: your value (null = the config's) and what the config itself sets. */
export type TweakRow = { id: string; label: string; description: string; value: string | null; configValue: string | null };
export type FixRow = { id: string; label: string; on: boolean };
export type TweaksView = { rows: TweakRow[]; fixes: FixRow[] };

export type ApplyResponse =
  | { kind: "preview"; preview: Preview }
  | { kind: "done"; message: string; notes: string[] };

export type BackupMeta = {
  id: string;
  createdAt: string;
  original: boolean;
  packName: string | null;
  reason: string;
  gameinfoPath: string | null;
  videoPath: string | null;
  absent: string[];
};

/** Only "installer" copies update themselves; Scoop and portable copies are updated outside the app. */
export type InstallKind = "installer" | "scoop" | "portable";

export type PathKind = "steam" | "gameinfo" | "video";
export type Location = "game" | "video" | "userdata" | "backups";

let storeOnce: Promise<StoreView> | null = null;

export const api = {
  overview: () => invoke<Overview>("get_overview"),
  setAccount: (id: string) => invoke<Overview>("set_account", { id }),
  setPath: (kind: PathKind, path: string | null) => invoke<Overview>("set_path", { kind, path }),
  saveSettings: (preservedKeys: string[]) => invoke<Overview>("save_settings", { preservedKeys }),
  listPacks: () => invoke<RepoGroup[]>("list_packs"),
  /** Last saved pack lists from disk; instant, no network. */
  cachedPacks: () => invoke<RepoGroup[]>("cached_packs"),
  addRepo: (input: string) => invoke<{ overview: Overview; group: RepoGroup }>("add_repo", { input }),
  removeRepo: (repo: string) => invoke<Overview>("remove_repo", { repo }),
  moveRepo: (repo: string, delta: number) => invoke<Overview>("move_repo", { repo, delta }),
  applyPack: (pack: PackInfo, confirmed: boolean, rankedSafe: boolean) =>
    invoke<ApplyResponse>("apply_pack", { pack, confirmed, rankedSafe }),
  applyLocal: (folder: string, confirmed: boolean, rankedSafe: boolean) =>
    invoke<ApplyResponse>("apply_local", { folder, confirmed, rankedSafe }),
  makeRankedSafe: (confirmed: boolean) => invoke<ApplyResponse>("make_ranked_safe", { confirmed }),
  listBackups: () => invoke<BackupMeta[]>("list_backups"),
  restore: (id: string) => invoke<string>("restore_backup", { id }),
  open: (which: Location) => invoke<void>("open_location", { which }),
  openGithub: (url: string) => invoke<void>("open_github", { url }),
  togglePin: (key: string) => invoke<Overview>("toggle_pin", { key }),
  diagnostics: () => invoke<string>("get_diagnostics"),
  finishGuide: () => invoke<Overview>("finish_guide"),
  getTweaks: () => invoke<TweaksView>("get_tweaks"),
  setTweak: (id: string, value: string | null) => invoke<TweaksView>("set_tweak", { id, value }),
  setFix: (id: string, on: boolean) => invoke<TweaksView>("set_fix", { id, on }),
  listLibrary: () => invoke<LibraryEntry[]>("list_library"),
  saveCurrent: (name: string, notes: string) => invoke<LibraryEntry>("save_current", { name, notes }),
  importConfig: (path: string) => invoke<LibraryEntry>("import_config", { path }),
  exportConfig: (id: string, path: string) => invoke<void>("export_config", { id, path }),
  deleteConfig: (id: string) => invoke<LibraryEntry[]>("delete_config", { id }),
  applyLibrary: (id: string, confirmed: boolean, rankedSafe: boolean) =>
    invoke<ApplyResponse>("apply_library", { id, confirmed, rankedSafe }),
  sharePack: (pack: PackInfo, rankedSafe: boolean) => invoke<string>("share_pack", { pack, rankedSafe }),
  importCode: (text: string) => invoke<LibraryEntry>("import_code", { text }),
  importGamebanana: (url: string) => invoke<LibraryEntry>("import_gamebanana", { url }),
  getStore: () => invoke<StoreView>("get_store"),
  /** The store list, fetched once per app start (for the suggestions under the add-repo field). */
  getStoreOnce: (): Promise<StoreView> => (storeOnce ??= invoke<StoreView>("get_store").catch((e) => {
    storeOnce = null;
    throw e;
  })),
  githubStatus: () => invoke<GithubStatus>("github_status"),
  getPerf: () => invoke<PerfView>("get_perf"),
  clearPerf: () => invoke<void>("clear_perf"),
  githubSignIn: () => invoke<DeviceCode>("github_sign_in"),
  githubPoll: (deviceCode: string, interval: number) => invoke<PollResult>("github_poll", { deviceCode, interval }),
  githubSignOut: () => invoke<void>("github_sign_out"),
  getAutomation: () => invoke<Automation>("get_automation"),
  updateReport: () => invoke<UpdateReport | null>("update_report"),
  setAutomation: (a: Automation) => invoke<Automation>("set_automation", a),
  installKind: () => invoke<InstallKind>("install_kind"),
};

/** Pack id prefix of an applied saved config. */
export const LIBRARY_PREFIX = "lib:";

export const ISSUES_URL = "https://github.com/alzzzik/deadlock-config-manager/issues/new/choose";

/** Tauri rejects with the Rust error string; normalise anything else. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

const RELATIVE = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

/** "today", "yesterday", "3 weeks ago", … */
export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const minutes = Math.round((t - Date.now()) / 60_000);
  if (minutes > -60) return minutes > -2 ? "just now" : RELATIVE.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (hours > -24) return RELATIVE.format(hours, "hour");
  const days = Math.round(hours / 24);
  if (days > -14) return RELATIVE.format(days, "day");
  if (days > -60) return RELATIVE.format(Math.round(days / 7), "week");
  return RELATIVE.format(Math.round(days / 30), "month");
}

const DAY_MS = 86_400_000;
/** Store entries updated within this many days count as recently updated (about 3 months). */
export const RECENT_DAYS = 92;

/** Whole days from a YYYY-MM-DD date to `now`, or null for a missing or bad date. */
export function daysSince(day: string | null | undefined, now = Date.now()): number | null {
  const t = day ? Date.parse(`${day}T00:00:00Z`) : NaN;
  return Number.isNaN(t) ? null : Math.max(0, Math.floor((now - t) / DAY_MS));
}

/** "today", "yesterday", "3 weeks ago" for a YYYY-MM-DD date. */
export function dayAgo(day: string | null | undefined, now = Date.now()): string {
  const days = daysSince(day, now);
  if (days === null) return "";
  if (days < 14) return RELATIVE.format(-days, "day");
  if (days < 60) return RELATIVE.format(-Math.round(days / 7), "week");
  return RELATIVE.format(-Math.round(days / 30), "month");
}

export const storeUpdated = (e: StoreEntry): string | null => (e.kind === "code" ? null : e.updated);

/** Newest first; entries without a date keep their order at the end. */
export const sortByUpdated = (entries: StoreEntry[]): StoreEntry[] =>
  [...entries].sort((a, b) => (storeUpdated(b) ?? "").localeCompare(storeUpdated(a) ?? ""));

export function isRecent(e: StoreEntry, now = Date.now()): boolean {
  const days = daysSince(storeUpdated(e), now);
  return days !== null && days <= RECENT_DAYS;
}

export function shortDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** "2026-10-20_02-10-05" → readable local time. */
export function formatBackupId(id: string): string {
  const m = id.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})-(\d{2})/);
  if (!m) return id;
  const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]);
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export const isSamePack = (a: AppliedRecord | null, p: PackInfo, firstRepo: string | undefined): boolean =>
  !!a && a.packId === p.id && (a.repo ?? firstRepo) === p.repo;

/** Settings key for a pinned pack. */
export const pinKey = (p: PackInfo): string => `${p.repo}#${p.id}`;

/** Case-insensitive match on the text a user would search for. */
export function matchesSearch(p: PackInfo, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [p.name, p.description, p.repo, p.commitMessage ?? ""].some((t) => t.toLowerCase().includes(q));
}

/** r_aspectratio → field of view, from the values players use (1.33 = 70 … 3.5 = 120); straight lines between them. */
const FOV_POINTS: [number, number][] = [[1.33, 70], [1.56, 75], [1.75, 80], [2.0, 85], [2.15, 90], [2.5, 100], [3.0, 110], [3.5, 120]];

export function fovFor(aspect: number): number | null {
  if (!Number.isFinite(aspect) || aspect <= 0) return null;
  const found = FOV_POINTS.findIndex(([a]) => a >= aspect);
  // Below or above the table, extend the first or last segment.
  const i = found === -1 ? FOV_POINTS.length - 1 : Math.max(1, found);
  const [a0, f0] = FOV_POINTS[i - 1];
  const [a1, f1] = FOV_POINTS[i];
  return Math.round(f0 + ((aspect - a0) / (a1 - a0)) * (f1 - f0));
}
