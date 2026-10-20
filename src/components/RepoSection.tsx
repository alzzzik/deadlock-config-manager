import { useEffect, useRef, useState } from "react";
import { AppliedRecord, PackInfo, RepoGroup, isSamePack, pinKey, timeAgo } from "../api";
import { PackCard } from "./PackCard";

interface RepoSectionProps {
  group: RepoGroup;
  /** Packs to show (the group's packs, filtered by search). */
  packs: PackInfo[];
  pinned: string[];
  index: number;
  count: number;
  firstRepo: string | undefined;
  applied: AppliedRecord | null;
  disabled: boolean;
  loading: boolean;
  onApply: (p: PackInfo) => void;
  onRefresh: () => void;
  onOpenGithub: (url: string) => void;
  onMove: (repo: string, delta: number) => void;
  onRemove: (repo: string) => void;
  onTogglePin: (p: PackInfo) => void;
}

function RepoMenu({ items }: { items: { label: string; onClick: () => void; danger?: boolean; disabled?: boolean }[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  return (
    <div className="menu" ref={ref}>
      <button className="icon-btn" aria-label="Repo options" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        ⋯
      </button>
      {open && (
        <ul className="menu-list" role="menu">
          {items.map((it) => (
            <li key={it.label}>
              <button
                role="menuitem"
                className={it.danger ? "danger" : ""}
                disabled={it.disabled}
                onClick={() => {
                  setOpen(false);
                  it.onClick();
                }}
              >
                {it.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function RepoSection({ group: g, packs, pinned, index, count, firstRepo, applied, disabled, loading, onApply, onRefresh, onOpenGithub, onMove, onRemove, onTogglePin }: RepoSectionProps) {
  const packCount = `${g.packs.length} pack${g.packs.length === 1 ? "" : "s"}`;
  return (
    <section className="repo" aria-label={g.repo}>
      <header className="repo-head">
        <GithubMark />
        <button className="repo-name" onClick={() => onOpenGithub(g.url)} disabled={!g.url} title="Open on GitHub">
          {g.repo}
        </button>
        <span className="muted small">
          {g.updatedAt ? `updated ${timeAgo(g.updatedAt)} · ` : ""}
          {packCount}
        </span>
        {g.offline && <span className="tag tag-warn">offline · cached</span>}
        <span className="spacer" />
        <button className="icon-btn" onClick={onRefresh} disabled={loading} aria-label={`Refresh ${g.repo}`} title="Refresh">
          <span className={loading ? "spin" : ""}>↻</span>
        </button>
        <RepoMenu
          items={[
            { label: "Open on GitHub", onClick: () => onOpenGithub(g.url), disabled: !g.url },
            { label: "Move up", onClick: () => onMove(g.repo, -1), disabled: index === 0 },
            { label: "Move down", onClick: () => onMove(g.repo, 1), disabled: index === count - 1 },
            { label: "Remove", onClick: () => onRemove(g.repo), danger: true },
          ]}
        />
      </header>

      {g.error && !g.offline && (
        <p className="banner banner-bad">
          {g.error}{" "}
          <button className="link-btn" onClick={onRefresh}>
            Retry
          </button>
        </p>
      )}

      <div className="pack-grid">
        {packs.map((p, i) => {
          const isApplied = isSamePack(applied, p, firstRepo);
          const hasUpdate = isApplied && !!applied?.revision && !!p.revision && applied.revision !== p.revision;
          return (
            <PackCard
              key={p.id}
              pack={p}
              index={i}
              applied={isApplied}
              hasUpdate={hasUpdate}
              disabled={disabled}
              pinned={pinned.includes(pinKey(p))}
              onApply={onApply}
              onOpenGithub={onOpenGithub}
              onTogglePin={onTogglePin}
            />
          );
        })}
      </div>
    </section>
  );
}

function GithubMark() {
  return (
    <svg className="gh" width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}
