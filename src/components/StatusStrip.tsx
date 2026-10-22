import { useEffect, useState } from "react";
import { Location, Overview, PathKind, Status, UpdateReport, api, timeAgo } from "../api";
import { AccountHelp } from "./AccountHelp";
import { CheckIcon, ChevronIcon, CloseIcon, FolderIcon, PlusIcon } from "./icons";

interface StatusStripProps {
  overview: Overview;
  busy: boolean;
  updateAvailable: boolean;
  /** Latest commit message of the applied pack when an update is available. */
  updateNote: string | null;
  canReapply: boolean;
  onReapply: () => void;
  onMakeRankedSafe: () => void;
  onAccount: (id: string) => void;
  onLocate: (kind: PathKind) => void;
  onAuto: (kind: PathKind) => void;
  onOpen: (which: Location) => void;
}

type Tone = "good" | "warn" | "bad" | "idle";
type FileState = "ok" | "new" | "bad";

const PATHS_OPEN_KEY = "dcm.pathsOpen";

function readPathsOpen(): boolean {
  try {
    return localStorage.getItem(PATHS_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

function writePathsOpen(open: boolean) {
  try {
    localStorage.setItem(PATHS_OPEN_KEY, open ? "1" : "0");
  } catch {
    // Storage unavailable: the toggle still works for this session.
  }
}

function headline(s: Status, o: Overview, update: boolean): { tone: Tone; title: string; detail: string } {
  const a = o.applied;
  const source = a ? [a.repo, a.appliedAt && `applied ${timeAgo(a.appliedAt)}`].filter(Boolean).join(" · ") : "";
  switch (s.kind) {
    case "applied":
    case "appliedDmm":
      return {
        tone: update ? "warn" : "good",
        title: s.pack,
        detail: [source, a?.rankedSafe && "ranked-safe", s.kind === "appliedDmm" && "edited by Deadlock Mod Manager"].filter(Boolean).join(" · "),
      };
    case "changed":
      return { tone: "warn", ...changedText(o.changedBy, s.pack) };
    case "missing":
      return o.gameinfoCreatable
        ? { tone: "idle", title: "Ready", detail: "gameinfo.gi is missing; applying a pack puts one in place." }
        : { tone: "bad", title: "Deadlock not found", detail: "Locate your install under Paths." };
    case "notApplied":
      return { tone: "idle", title: "No pack applied", detail: "Pick a pack below to apply it." };
  }
}

function changedText(by: Overview["changedBy"], pack: string): { title: string; detail: string } {
  switch (by) {
    case "dmm":
      return { title: "Changed by Deadlock Mod Manager", detail: `Mod Manager rewrote gameinfo.gi, so ${pack} is no longer fully applied.` };
    case "grimoire":
      return { title: "Changed by Grimoire", detail: `Grimoire rewrote gameinfo.gi, so ${pack} is no longer fully applied.` };
    case "other":
      return { title: "gameinfo.gi was changed", detail: `Another program or a manual edit changed it, so ${pack} is no longer applied.` };
    default:
      return { title: "Game updated", detail: `Deadlock replaced gameinfo.gi, so ${pack} is no longer applied.` };
  }
}

const STATE_ICON: Record<FileState, JSX.Element> = {
  ok: <CheckIcon size={13} />,
  new: <PlusIcon size={13} />,
  bad: <CloseIcon size={13} />,
};

function FileChip({ name, state }: { name: string; state: FileState }) {
  const label = state === "ok" ? "found" : state === "new" ? "will be created" : "missing";
  return (
    <span className={`file-chip is-${state}`} title={`${name}: ${label}`}>
      {STATE_ICON[state]}
      {name}
    </span>
  );
}

interface PathRowProps {
  name: string;
  path: string | null;
  state: FileState;
  note?: string;
  onLocate: () => void;
  onAuto: () => void;
  onOpen: () => void;
}

function PathRow({ name, path, state, note, onLocate, onAuto, onOpen }: PathRowProps) {
  return (
    <div className="path-row">
      <span className="path-name">{name}</span>
      <div className="path-value">
        <code title={path ?? ""}>{path ?? "Not found"}</code>
        {note && <small className={`path-note is-${state}`}>{note}</small>}
      </div>
      <div className="path-actions">
        <button className="btn btn-quiet btn-sm" onClick={onLocate}>
          Locate…
        </button>
        <button className="btn btn-quiet btn-sm" onClick={onAuto} title="Forget the manual path and detect it again">
          Auto
        </button>
        <button className="icon-btn" onClick={onOpen} disabled={!path} aria-label={`Show ${name} in Explorer`} title="Show in Explorer">
          <FolderIcon />
        </button>
      </div>
    </div>
  );
}

export function StatusStrip(props: StatusStripProps) {
  const { overview: o, busy, updateAvailable, updateNote, canReapply, onReapply, onMakeRankedSafe, onAccount, onLocate, onAuto, onOpen } = props;
  const h = headline(o.status, o, updateAvailable);
  const gameState: FileState = o.gameinfoFound ? "ok" : o.gameinfoCreatable ? "new" : "bad";
  const videoState: FileState = o.videoFound ? "ok" : o.videoPath ? "new" : "bad";
  const hasProblem = gameState === "bad" || videoState === "bad";
  const [pathsOpen, setPathsOpen] = useState(() => readPathsOpen());

  // Open the paths automatically when something needs fixing.
  useEffect(() => {
    if (hasProblem) setPathsOpen(true);
  }, [hasProblem]);

  const togglePaths = () => {
    setPathsOpen((v) => {
      writePathsOpen(!v);
      return !v;
    });
  };

  const showReapply = canReapply && (o.status.kind === "changed" || updateAvailable);
  const isApplied = o.status.kind === "applied" || o.status.kind === "appliedDmm";

  return (
    <section className={`status tone-${h.tone}`} aria-labelledby="status-title">
      <div className="status-main">
        <span className="status-dot" aria-hidden />
        <div className="status-text">
          <h1 id="status-title">
            {isApplied && <span className="status-prefix">Applied</span>}
            {h.title}
          </h1>
          <p>
            {updateAvailable ? (
              <>
                <span className="status-update">Update available</span>
                {updateNote ? ` · ${updateNote}` : ""}
              </>
            ) : (
              h.detail
            )}
          </p>
        </div>
        {showReapply && (
          <button className="btn btn-primary" disabled={busy || o.deadlockRunning} onClick={onReapply}>
            {updateAvailable ? "Update" : "Re-apply"}
          </button>
        )}
      </div>

      {o.status.kind === "changed" && o.changedBy === "game" && <UpdateReportNotice gameinfoKey={o.gameBuild ?? ""} />}
      {o.deadlockRunning && <p className="notice is-warn">Deadlock is running. Close it before applying or restoring.</p>}
      {!!o.rankedChanges && o.status.kind !== "missing" && (
        <div className="notice is-warn notice-action">
          <span>
            Your gameinfo.gi changes {o.rankedChanges} engine setting{o.rankedChanges === 1 ? "" : "s"} that Valve checks
            before ranked matchmaking, so ranked queueing may be blocked.
          </span>
          <button className="btn btn-secondary btn-sm" disabled={busy || o.deadlockRunning} onClick={onMakeRankedSafe}>
            Make ranked-safe
          </button>
        </div>
      )}

      <div className="status-files">
        <FileChip name="gameinfo.gi" state={gameState} />
        <FileChip name="video.txt" state={videoState} />
        <span className="files-sep" aria-hidden />
        {o.accounts.length > 0 ? (
          <label className="account">
            <span>Account</span>
            <select value={o.accountId ?? ""} onChange={(e) => onAccount(e.target.value)}>
              {o.accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {(a.persona ?? a.id) + (a.hasVideo ? "" : " (no video.txt yet)")}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <span className="muted">No Steam account found</span>
        )}
        <AccountHelp onOpenUserdata={() => onOpen("userdata")} />
        <button className="paths-toggle" onClick={togglePaths} aria-expanded={pathsOpen} aria-controls="paths-panel">
          Paths
          <ChevronIcon size={12} className="chev" />
        </button>
      </div>

      {pathsOpen && (
        <div className="paths" id="paths-panel">
          <PathRow
            name="gameinfo.gi"
            path={o.gameinfoPath}
            state={gameState}
            note={gameState === "new" ? "Missing. Applying a pack creates it." : gameState === "bad" ? "Deadlock install not found" : undefined}
            onLocate={() => onLocate("gameinfo")}
            onAuto={() => onAuto("gameinfo")}
            onOpen={() => onOpen("game")}
          />
          <PathRow
            name="video.txt"
            path={o.videoPath}
            state={videoState}
            note={videoState === "new" ? "Not created yet. Applying a pack creates it." : videoState === "bad" ? "No Steam account found" : undefined}
            onLocate={() => onLocate("video")}
            onAuto={() => onAuto("video")}
            onOpen={() => onOpen("video")}
          />
          <PathRow
            name="Steam"
            path={o.steamRoot}
            state={o.steamRoot ? "ok" : "bad"}
            onLocate={() => onLocate("steam")}
            onAuto={() => onAuto("steam")}
            onOpen={() => onOpen("userdata")}
          />
        </div>
      )}
    </section>
  );
}

/** After a game update: what changed for the applied config. */
function UpdateReportNotice({ gameinfoKey }: { gameinfoKey: string }) {
  const [r, setR] = useState<UpdateReport | null>(null);
  useEffect(() => {
    api.updateReport().then(setR, () => setR(null));
  }, [gameinfoKey]);
  if (!r) return null;
  const items = [
    r.newSettings.length > 0 &&
      `Valve added ${r.newSettings.length} setting${r.newSettings.length === 1 ? "" : "s"} your config doesn't have; re-applying removes them unless overlay mode is on (Settings).`,
    r.ignored.length > 0 && `${r.ignored.length} setting${r.ignored.length === 1 ? "" : "s"} in your config ${r.ignored.length === 1 ? "is" : "are"} ignored by the game.`,
    r.engineChanges > 0 && `Your config differs from Valve's new file in ${r.engineChanges} engine setting${r.engineChanges === 1 ? "" : "s"}, which can block ranked; re-apply it ranked-safe to be sure.`,
  ].filter((x): x is string => !!x);
  if (items.length === 0) return null;
  const builds = r.fromBuild && r.toBuild && r.fromBuild !== r.toBuild ? ` (build ${r.fromBuild} → ${r.toBuild})` : "";
  return (
    <div className="notice is-warn update-report">
      <strong>What this update changed{builds}</strong>
      <ul>
        {items.map((t) => (
          <li key={t}>{t}</li>
        ))}
      </ul>
    </div>
  );
}
