import { useEffect, useMemo, useState } from "react";
import { StoreEntry, StoreView, api, dayAgo, errorText, isRecent, shortDate, sortByUpdated, storeUpdated } from "../api";
import { ExternalIcon } from "./icons";
import { Modal } from "./Modal";

interface StoreDialogProps {
  linked: string[];
  busy: boolean;
  onClose: () => void;
  onLinkRepo: (repo: string) => Promise<string | null>;
  onAddCode: (code: string) => void;
  onAddGamebanana: (url: string) => void;
  onOpenGithub: (url: string) => void;
}

const gamebananaUrl = (id: number) => `https://gamebanana.com/mods/${id}`;

const entryKey = (e: StoreEntry) => (e.kind === "repo" ? e.repo : e.kind === "gamebanana" ? `gb${e.id}` : e.code);

const searchText = (e: StoreEntry) =>
  [e.title, e.description, e.author, e.kind === "repo" ? `${e.repo} ${e.configs.join(" ")}` : ""].join(" ").toLowerCase();

const BADGE: Record<StoreEntry["kind"], string> = { repo: "GitHub", gamebanana: "GameBanana", code: "Shared config" };

/** Browse config repos, GameBanana configs and shared configs, newest first. */
export function StoreDialog({ linked, busy, onClose, onLinkRepo, onAddCode, onAddGamebanana, onOpenGithub }: StoreDialogProps) {
  const [view, setView] = useState<StoreView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [recentOnly, setRecentOnly] = useState(true);

  useEffect(() => {
    api.getStore().then(setView, (e) => setError(errorText(e)));
  }, []);

  const sorted = useMemo(() => sortByUpdated(view?.entries ?? []), [view]);
  const recentCount = useMemo(() => sorted.filter((e) => isRecent(e)).length, [sorted]);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sorted.filter((e) => (!recentOnly || isRecent(e) || e.kind === "code") && (!q || searchText(e).includes(q)));
  }, [sorted, query, recentOnly]);

  const isLinked = (e: StoreEntry) => e.kind === "repo" && linked.some((r) => r.toLowerCase() === e.repo.toLowerCase());

  const link = async (repo: string) => {
    const err = await onLinkRepo(repo);
    if (err) setError(err);
  };

  const sourceUrl = (e: StoreEntry) => (e.kind === "repo" ? `https://github.com/${e.repo}` : e.kind === "gamebanana" ? gamebananaUrl(e.id) : null);

  return (
    <Modal
      title="Browse configs"
      onClose={onClose}
      wide
      footer={
        <>
          <span className="muted small">
            {view?.online ? "Online list" : "Built-in list (the online list isn't reachable)"}
            {view?.checked ? `, dates checked ${shortDate(view.checked)}.` : "."}
          </span>
          <span className="spacer" />
          {view && (
            <button className="btn btn-quiet" onClick={() => onOpenGithub(view.submitUrl)}>
              Submit a config…
            </button>
          )}
          <button className="btn btn-secondary" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      {error && <p className="notice is-bad">{error}</p>}
      <div className="store-tools">
        <label className="search store-search">
          <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search configs" aria-label="Search configs" />
        </label>
        <label className="store-recent">
          <input type="checkbox" checked={recentOnly} onChange={(e) => setRecentOnly(e.target.checked)} />
          Updated in the last 3 months
        </label>
      </div>
      {view && (
        <p className="muted small store-count">
          {recentOnly ? `${recentCount} of ${sorted.length} updated recently, newest first.` : `${sorted.length} configs, newest first.`}
        </p>
      )}
      {!view && !error && <p className="muted">Loading…</p>}
      <ul className="pack-list store-list">
        {shown.map((e) => {
          const updated = storeUpdated(e);
          const url = sourceUrl(e);
          return (
            <li key={entryKey(e)} className="pack-row store-row">
              <div className="pack-main">
                <div className="pack-title">
                  <h3>{e.title}</h3>
                  <span className="badge">{BADGE[e.kind]}</span>
                </div>
                <p className="pack-desc" title={e.description}>
                  {e.description || "No description"}
                </p>
                {e.kind === "repo" && e.configs.length > 1 && (
                  <p className="pack-desc store-configs" title={e.configs.join(", ")}>
                    {e.configs.length} configs: {e.configs.join(", ")}
                  </p>
                )}
              </div>
              <div className="pack-meta">
                <span title={e.kind === "repo" ? e.repo : undefined}>{e.kind === "repo" ? e.repo : e.author ? `by ${e.author}` : ""}</span>
                {updated && (
                  <span className={isRecent(e) ? undefined : "is-stale"} title={updated}>
                    {isRecent(e) ? `Updated ${dayAgo(updated)}` : `No update since ${shortDate(updated)}`}
                  </span>
                )}
              </div>
              <div className="pack-actions">
                {url && (
                  <button className="icon-btn" onClick={() => onOpenGithub(url)} aria-label={`View ${e.title}`} title={e.kind === "repo" ? "View on GitHub" : "View on GameBanana"}>
                    <ExternalIcon size={14} />
                  </button>
                )}
                {e.kind === "repo" ? (
                  isLinked(e) ? (
                    <span className="muted small">Linked</span>
                  ) : (
                    <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => void link(e.repo)}>
                      Link
                    </button>
                  )
                ) : (
                  <button
                    className="btn btn-secondary btn-sm"
                    disabled={busy}
                    onClick={() => (e.kind === "gamebanana" ? onAddGamebanana(gamebananaUrl(e.id)) : onAddCode(e.code))}
                    title="Adds it to My configs, where you can preview it before installing"
                  >
                    Add to My configs
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      {view && shown.length === 0 && (
        <p className="muted">{recentOnly && !query ? "Nothing updated recently. Untick the filter to see older configs." : "Nothing matches."}</p>
      )}
    </Modal>
  );
}
