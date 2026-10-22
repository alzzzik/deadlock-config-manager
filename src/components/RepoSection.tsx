import { ReactNode, useEffect, useRef, useState } from "react";
import { AppliedRecord, PackInfo, RepoGroup, isSamePack, pinKey, timeAgo } from "../api";
import { PackRow } from "./PackRow";
import { GithubIcon, MoreIcon, PinIcon, RefreshIcon } from "./icons";

interface MenuItem {
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export function RepoMenu({ label, items }: { label: string; items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div className="menu" ref={ref}>
      <button className="icon-btn" aria-label={label} aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <MoreIcon />
      </button>
      {open && (
        <ul className="menu-list" role="menu">
          {items.map((it) => (
            <li key={it.label}>
              <button
                role="menuitem"
                className={it.danger ? "is-danger" : ""}
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

interface PackListProps {
  packs: PackInfo[];
  pinned: string[];
  firstRepo: string | undefined;
  applied: AppliedRecord | null;
  disabled: boolean;
  showRepo?: boolean;
  onApply: (p: PackInfo) => void;
  onOpenGithub: (url: string) => void;
  onTogglePin: (p: PackInfo) => void;
  onShare?: (p: PackInfo) => void;
}

export function PackList({ packs, pinned, firstRepo, applied, disabled, showRepo, onApply, onOpenGithub, onTogglePin, onShare }: PackListProps) {
  return (
    <ul className="pack-list">
      {packs.map((p) => (
        <PackRow
          key={pinKey(p)}
          pack={p}
          applied={isSamePack(applied, p, firstRepo)}
          disabled={disabled}
          pinned={pinned.includes(pinKey(p))}
          showRepo={showRepo}
          onApply={onApply}
          onOpenGithub={onOpenGithub}
          onTogglePin={onTogglePin}
          onShare={onShare}
        />
      ))}
    </ul>
  );
}

/** Group heading shared by repo groups and the Pinned group. */
export function GroupHead({ icon, title, meta, children }: { icon: ReactNode; title: ReactNode; meta?: ReactNode; children?: ReactNode }) {
  return (
    <header className="group-head">
      <span className="group-icon">{icon}</span>
      <span className="group-title">{title}</span>
      {meta && <span className="group-meta">{meta}</span>}
      <span className="spacer" />
      {children}
    </header>
  );
}

export function PinnedSection(props: Omit<PackListProps, "showRepo">) {
  return (
    <section className="group" aria-label="Pinned packs">
      <GroupHead icon={<PinIcon size={14} filled />} title="Pinned" />
      <PackList {...props} showRepo />
    </section>
  );
}

interface RepoSectionProps extends Omit<PackListProps, "packs" | "showRepo"> {
  group: RepoGroup;
  /** Packs to show (the group's packs, filtered by search). */
  packs: PackInfo[];
  index: number;
  count: number;
  loading: boolean;
  onRefresh: () => void;
  onMove: (repo: string, delta: number) => void;
  onRemove: (repo: string) => void;
}

export function RepoSection({ group: g, index, count, loading, onRefresh, onMove, onRemove, ...list }: RepoSectionProps) {
  const packCount = `${g.packs.length} pack${g.packs.length === 1 ? "" : "s"}`;
  return (
    <section className="group" aria-label={g.repo}>
      <GroupHead
        icon={<GithubIcon size={14} />}
        title={
          <button className="group-link" onClick={() => list.onOpenGithub(g.url)} disabled={!g.url} title="Open on GitHub">
            {g.repo}
          </button>
        }
        meta={
          <>
            {g.updatedAt ? `Updated ${timeAgo(g.updatedAt)} · ` : ""}
            {packCount}
            {g.offline && <span className="badge is-warn">Offline, cached</span>}
          </>
        }
      >
        <button className="icon-btn" onClick={onRefresh} disabled={loading} aria-label={`Refresh ${g.repo}`} title="Refresh">
          <RefreshIcon className={loading ? "spin" : ""} />
        </button>
        <RepoMenu
          label={`Options for ${g.repo}`}
          items={[
            { label: "Open on GitHub", onClick: () => list.onOpenGithub(g.url), disabled: !g.url },
            { label: "Move up", onClick: () => onMove(g.repo, -1), disabled: index === 0 },
            { label: "Move down", onClick: () => onMove(g.repo, 1), disabled: index === count - 1 },
            { label: "Remove", onClick: () => onRemove(g.repo), danger: true },
          ]}
        />
      </GroupHead>

      {g.error && !g.offline && (
        <p className="notice is-bad">
          {g.error}
          <button className="btn btn-quiet btn-sm" onClick={onRefresh}>
            Retry
          </button>
        </p>
      )}

      <PackList {...list} />
    </section>
  );
}
