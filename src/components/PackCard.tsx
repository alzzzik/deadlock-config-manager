import { PackInfo, timeAgo } from "../api";

interface PackCardProps {
  pack: PackInfo;
  applied: boolean;
  hasUpdate: boolean;
  disabled: boolean;
  index: number;
  pinned: boolean;
  /** Shown on cards in the Pinned row, where the repo isn't obvious from context. */
  showRepo?: boolean;
  onApply: (p: PackInfo) => void;
  onOpenGithub: (url: string) => void;
  onTogglePin: (p: PackInfo) => void;
}

/** A pack folder untouched this long may not match the current game version. */
const STALE_DAYS = 90;
const DAY_MS = 86_400_000;

function ageInDays(iso: string | null): number | null {
  if (!iso) return null;
  const t = new Date(iso).getTime();
  return Number.isNaN(t) ? null : Math.floor((Date.now() - t) / DAY_MS);
}

export function PackCard({ pack: p, applied, hasUpdate, disabled, index, pinned, showRepo, onApply, onOpenGithub, onTogglePin }: PackCardProps) {
  const days = ageInDays(p.lastCommitDate);
  const stale = days !== null && days >= STALE_DAYS;
  const fullDate = p.lastCommitDate ? new Date(p.lastCommitDate).toLocaleString() : undefined;

  return (
    <article className={`pack-card ${applied ? "is-applied" : ""}`} style={{ animationDelay: `${index * 50}ms` }}>
      <div className="pack-top">
        <h3>{p.name}</h3>
        {applied && <span className={`tag ${hasUpdate ? "tag-warn" : "tag-good"}`}>{hasUpdate ? "Update" : "Applied"}</span>}
        <span className="spacer" />
        <button
          className={`star ${pinned ? "is-pinned" : ""}`}
          onClick={() => onTogglePin(p)}
          aria-pressed={pinned}
          aria-label={pinned ? `Unpin ${p.name}` : `Pin ${p.name}`}
          title={pinned ? "Unpin" : "Pin to top"}
        >
          {pinned ? "★" : "☆"}
        </button>
      </div>
      {showRepo && <p className="pack-repo muted small">{p.repo}</p>}
      <p className="pack-desc">{p.description || "No description."}</p>
      {p.commitMessage && (
        <p className="pack-commit" title={p.commitMessage}>
          <span className="commit-dot" aria-hidden />
          <span className="commit-msg">{p.commitMessage}</span>
        </p>
      )}
      <p className={`pack-updated ${stale ? "is-stale" : ""}`} title={fullDate}>
        {days === null ? (
          <span className="muted">Last update unknown</span>
        ) : (
          <>
            <span>Updated {timeAgo(p.lastCommitDate)}</span>
            {stale && <span className="tag tag-warn">Not updated in {Math.round(days / 30)} months</span>}
          </>
        )}
      </p>
      <div className="pack-foot">
        <button className="link-btn" onClick={() => onOpenGithub(p.htmlUrl)} disabled={!p.htmlUrl}>
          View on GitHub ↗
        </button>
        <span className="spacer" />
        <button className="btn btn-outline" disabled={disabled} onClick={() => onApply(p)}>
          {applied ? "Re-apply" : "Apply"}
        </button>
      </div>
    </article>
  );
}
