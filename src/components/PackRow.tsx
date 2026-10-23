import { PackInfo, timeAgo } from "../api";
import { GithubIcon, PinIcon, ShareIcon } from "./icons";

interface PackRowProps {
  pack: PackInfo;
  applied: boolean;
  disabled: boolean;
  pinned: boolean;
  /** Shown in the Pinned group, where the repo isn't obvious from context. */
  showRepo?: boolean;
  onApply: (p: PackInfo) => void;
  onOpenGithub: (url: string) => void;
  onTogglePin: (p: PackInfo) => void;
  onShare?: (p: PackInfo) => void;
}

/** A pack folder untouched this long may not match the current game version. */
const STALE_DAYS = 90;
const DAY_MS = 86_400_000;

function ageInDays(iso: string | null): number | null {
  if (!iso) return null;
  const t = new Date(iso).getTime();
  return Number.isNaN(t) ? null : Math.floor((Date.now() - t) / DAY_MS);
}

export function PackRow({ pack: p, applied, disabled, pinned, showRepo, onApply, onOpenGithub, onTogglePin, onShare }: PackRowProps) {
  const days = ageInDays(p.lastCommitDate);
  const stale = days !== null && days >= STALE_DAYS;
  const fullDate = p.lastCommitDate ? new Date(p.lastCommitDate).toLocaleString() : undefined;
  const description = p.description || "No description";

  return (
    <li className={`pack-row ${applied ? "is-applied" : ""} ${pinned ? "is-pinned" : ""}`}>
      <button
        className="row-pin"
        onClick={() => onTogglePin(p)}
        aria-pressed={pinned}
        aria-label={pinned ? `Unpin ${p.name}` : `Pin ${p.name}`}
        title={pinned ? "Unpin" : "Pin to top"}
      >
        <PinIcon size={14} filled={pinned} />
      </button>

      <div className="pack-main">
        <div className="pack-title">
          <h3>{p.name}</h3>
          {applied && <span className="badge is-good">Applied</span>}
        </div>
        <p className="pack-desc" title={description}>
          {showRepo && <span className="pack-repo">{p.repo} · </span>}
          {description}
        </p>
      </div>

      <div className="pack-meta" title={stale ? `Not updated since ${fullDate}. It may not match the current game version.` : fullDate}>
        <span className={stale ? "is-stale" : ""}>{days === null ? "Last update unknown" : `Updated ${timeAgo(p.lastCommitDate)}`}</span>
        {p.commitMessage && <span className="pack-commit">{p.commitMessage}</span>}
      </div>

      <div className="pack-actions">
        {onShare && (
          <button className="icon-btn row-hover" onClick={() => onShare(p)} aria-label={`Share ${p.name}`} title="Copy a share code (includes your tweaks)">
            <ShareIcon size={15} />
          </button>
        )}
        <button className="icon-btn row-hover" onClick={() => onOpenGithub(p.htmlUrl)} disabled={!p.htmlUrl} aria-label={`View ${p.name} on GitHub`} title="View on GitHub">
          <GithubIcon size={15} />
        </button>
        <button className="btn btn-secondary btn-sm" disabled={disabled} onClick={() => onApply(p)}>
          {applied ? "Re-apply" : "Apply"}
        </button>
      </div>
    </li>
  );
}
