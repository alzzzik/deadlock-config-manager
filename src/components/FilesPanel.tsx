import { ReactNode } from "react";
import { Location, Overview, PathKind, Status, timeAgo } from "../api";
import { AccountHelp } from "./AccountHelp";

interface FilesPanelProps {
  overview: Overview;
  busy: boolean;
  updateAvailable: boolean;
  canReapply: boolean;
  onReapply: () => void;
  onAccount: (id: string) => void;
  onLocate: (kind: PathKind) => void;
  onAuto: (kind: PathKind) => void;
  onOpen: (which: Location) => void;
}

function headline(s: Status, o: Overview): { tone: string; title: string } {
  switch (s.kind) {
    case "applied":
      return { tone: "good", title: `Applied: ${s.pack}` };
    case "appliedDmm":
      return { tone: "good", title: `Applied: ${s.pack}` };
    case "changed":
      return { tone: "warn", title: "Game updated or file changed" };
    case "missing":
      return o.gameinfoCreatable
        ? { tone: "idle", title: "Ready to apply" }
        : { tone: "bad", title: "Deadlock not found" };
    case "notApplied":
      return { tone: "idle", title: "No pack applied" };
  }
}

function detail(s: Status, o: Overview): string {
  const a = o.applied;
  const where = a ? [a.repo, a.appliedAt && `applied ${timeAgo(a.appliedAt)}`].filter(Boolean).join(" · ") : "";
  switch (s.kind) {
    case "applied":
      return where;
    case "appliedDmm":
      return `${where} · edited by Deadlock Mod Manager (that's fine)`;
    case "changed":
      return `gameinfo.gi no longer matches ${s.pack}. A Deadlock update usually does this.`;
    case "missing":
      return o.gameinfoCreatable
        ? "gameinfo.gi is missing; Apply will put the pack's file in place."
        : "Use Locate… next to gameinfo.gi to point at your install.";
    case "notApplied":
      return "Pick a pack below and click Apply.";
  }
}

interface FileRowProps {
  name: string;
  path: string | null;
  state: "ok" | "new" | "bad";
  note?: string;
  children: ReactNode;
}

function FileRow({ name, path, state, note, children }: FileRowProps) {
  return (
    <div className="file-row">
      <span className="file-name">
        <span className={`dot dot-${state}`} aria-hidden />
        {name}
      </span>
      <div className="file-path">
        <code title={path ?? ""}>{path ?? "Not found"}</code>
        {note && <small className={`file-note note-${state}`}>{note}</small>}
      </div>
      <div className="file-actions">{children}</div>
    </div>
  );
}

function LocateButtons({ kind, canOpen, onLocate, onAuto, onOpen }: { kind: string; canOpen: boolean; onLocate: () => void; onAuto: () => void; onOpen: () => void }) {
  return (
    <>
      <button className="btn btn-sm" onClick={onLocate}>
        Locate…
      </button>
      <button className="btn btn-sm btn-ghost" onClick={onAuto} title="Forget the manual path and detect it again">
        Auto
      </button>
      <button className="icon-btn" onClick={onOpen} disabled={!canOpen} aria-label={`Open the ${kind} folder in Explorer`} title="Show in Explorer">
        <FolderIcon />
      </button>
    </>
  );
}

function FolderIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

export function FilesPanel({ overview: o, busy, updateAvailable, canReapply, onReapply, onAccount, onLocate, onAuto, onOpen }: FilesPanelProps) {
  const h = headline(o.status, o);
  const showReapply = canReapply && (o.status.kind === "changed" || updateAvailable);
  const account = o.accounts.find((a) => a.id === o.accountId);
  const gameState = o.gameinfoFound ? "ok" : o.gameinfoCreatable ? "new" : "bad";
  const videoState = o.videoFound ? "ok" : o.videoPath ? "new" : "bad";

  return (
    <section className={`files-panel tone-${h.tone}`} aria-labelledby="status-title">
      <header className="status-head">
        <div>
          <p className="eyebrow">Status</p>
          <h1 id="status-title">
            {h.title}
            {updateAvailable && <span className="tag tag-warn">Update available</span>}
          </h1>
          <p className="status-sub">{detail(o.status, o)}</p>
        </div>
        {showReapply && (
          <button className="btn btn-primary" disabled={busy || o.deadlockRunning} onClick={onReapply}>
            Re-apply {o.applied?.packName}
          </button>
        )}
      </header>

      {o.deadlockRunning && <p className="banner banner-warn">Deadlock is running. Close it before applying or restoring.</p>}

      <div className="files">
        <FileRow
          name="gameinfo.gi"
          path={o.gameinfoPath}
          state={gameState}
          note={gameState === "new" ? "Missing. Apply will create it." : gameState === "bad" ? "Deadlock install not found" : undefined}
        >
          <LocateButtons kind="game" canOpen={!!o.gameinfoPath} onLocate={() => onLocate("gameinfo")} onAuto={() => onAuto("gameinfo")} onOpen={() => onOpen("game")} />
        </FileRow>

        <FileRow
          name="video.txt"
          path={o.videoPath}
          state={videoState}
          note={videoState === "new" ? "Not created yet. Apply will create it." : videoState === "bad" ? "No Steam account found" : undefined}
        >
          <LocateButtons kind="video" canOpen={!!o.videoPath} onLocate={() => onLocate("video")} onAuto={() => onAuto("video")} onOpen={() => onOpen("video")} />
        </FileRow>

        <div className="file-row">
          <span className="file-name">
            Steam account
            <AccountHelp onOpenUserdata={() => onOpen("userdata")} />
          </span>
          <div className="file-path">
            {o.accounts.length === 0 ? (
              <span className="muted">No Steam accounts found.</span>
            ) : (
              <>
                <select value={o.accountId ?? ""} onChange={(e) => onAccount(e.target.value)} aria-label="Steam account">
                  {o.accounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {(a.persona ? `${a.persona} · ${a.id}` : a.id) + (a.hasVideo ? "" : "  (no video.txt yet)")}
                    </option>
                  ))}
                </select>
                <small className="muted">
                  {o.accounts.length} account{o.accounts.length === 1 ? "" : "s"} found
                  {account && !account.hasVideo ? " · Deadlock hasn't run on this one yet" : ""}
                </small>
              </>
            )}
          </div>
          <div className="file-actions">
            <button className="btn btn-sm btn-ghost" onClick={() => onLocate("steam")}>
              Steam folder…
            </button>
            <button className="icon-btn" onClick={() => onOpen("userdata")} disabled={!o.steamRoot} aria-label="Open userdata folder" title="Open userdata folder">
              <FolderIcon />
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}
