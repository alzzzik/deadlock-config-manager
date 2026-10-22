import { useEffect, useMemo, useState } from "react";
import { StoreEntry, StoreView, api, errorText } from "../api";
import { Modal } from "./Modal";

interface StoreDialogProps {
  linked: string[];
  busy: boolean;
  onClose: () => void;
  onLinkRepo: (repo: string) => Promise<string | null>;
  onAddCode: (code: string) => void;
  onOpenGithub: (url: string) => void;
}

/** Browse config repos and shared configs from the community list. */
export function StoreDialog({ linked, busy, onClose, onLinkRepo, onAddCode, onOpenGithub }: StoreDialogProps) {
  const [view, setView] = useState<StoreView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    api.getStore().then(setView, (e) => setError(errorText(e)));
  }, []);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (view?.entries ?? []).filter((e) => !q || [e.title, e.description, e.author, e.kind === "repo" ? e.repo : ""].some((t) => t.toLowerCase().includes(q)));
  }, [view, query]);

  const isLinked = (e: StoreEntry) => e.kind === "repo" && linked.some((r) => r.toLowerCase() === e.repo.toLowerCase());

  const link = async (repo: string) => {
    const err = await onLinkRepo(repo);
    if (err) setError(err);
  };

  return (
    <Modal
      title="Browse configs"
      onClose={onClose}
      wide
      footer={
        <>
          <span className="muted small">
            {view && !view.online ? "Showing the built-in or last downloaded list (the online list isn't reachable)." : "Community list, added by pull request."}
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
      <label className="search store-search">
        <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search configs" aria-label="Search configs" />
      </label>
      {!view && !error && <p className="muted">Loading…</p>}
      <ul className="pack-list store-list">
        {shown.map((e) => (
          <li key={e.kind === "repo" ? e.repo : e.code} className="pack-row store-row">
            <div className="pack-main">
              <div className="pack-title">
                <h3>{e.title}</h3>
                <span className="badge">{e.kind === "repo" ? "Repo" : "Shared config"}</span>
              </div>
              <p className="pack-desc" title={e.description}>
                {e.description || "No description"}
              </p>
            </div>
            <div className="pack-meta">
              <span>{e.kind === "repo" ? e.repo : e.author ? `by ${e.author}` : ""}</span>
            </div>
            <div className="pack-actions">
              {e.kind === "repo" ? (
                isLinked(e) ? (
                  <span className="muted small">Linked</span>
                ) : (
                  <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => void link(e.repo)}>
                    Link
                  </button>
                )
              ) : (
                <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => onAddCode(e.code)}>
                  Add to My configs
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>
      {view && shown.length === 0 && <p className="muted">Nothing matches.</p>}
    </Modal>
  );
}
