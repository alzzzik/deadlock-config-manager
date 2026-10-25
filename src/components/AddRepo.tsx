import { FormEvent, useEffect, useState } from "react";
import { StoreEntry, api, dayAgo, isRecent, isRecommended, sortStore } from "../api";
import { ExternalIcon, PlusIcon } from "./icons";

/** How many recently updated repos from the config list are offered as one-click chips. */
const SUGGESTION_COUNT = 3;

interface AddRepoProps {
  /** Repos already linked, so their suggestions are hidden. */
  linked: string[];
  onAdd: (input: string) => Promise<string | null>;
  onOpenGithub: (url: string) => void;
  /** Opens the full config list (Browse). */
  onBrowse: () => void;
}

type RepoEntry = Extract<StoreEntry, { kind: "repo" }>;

/** Inline field for linking another pack repo. `onAdd` resolves to an error message or null. */
export function AddRepo({ linked, onAdd, onOpenGithub, onBrowse }: AddRepoProps) {
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const [catalog, setCatalog] = useState<StoreEntry[]>([]);

  useEffect(() => {
    // Suggestions are optional: without the list the field still works.
    let alive = true;
    api.getStoreOnce().then(
      (v) => alive && setCatalog(sortStore(v.entries)),
      () => alive && setCatalog([]),
    );
    return () => {
      alive = false;
    };
  }, []);

  const add = async (input: string) => {
    setAdding(input);
    const err = await onAdd(input);
    setAdding(null);
    setError(err);
    return err;
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!value.trim()) {
      setError("Paste a GitHub repo first, like owner/repo.");
      return;
    }
    if (!(await add(value.trim()))) setValue("");
  };

  const isLinked = (repo: string) => linked.some((r) => r.toLowerCase() === repo.toLowerCase());
  // Recommended repos first, then recently updated ones with several configs, then the rest (list order is
  // already recommended, then newest).
  const repos = catalog.filter((e): e is RepoEntry => e.kind === "repo" && (isRecommended(e) || isRecent(e)) && !isLinked(e.repo));
  const rank = (e: RepoEntry) => (isRecommended(e) ? 0 : e.configs.length > 1 ? 1 : 2);
  const suggestions = [...repos].sort((a, b) => rank(a) - rank(b)).slice(0, SUGGESTION_COUNT);

  return (
    <form className="add-repo" onSubmit={submit}>
      <div className="add-row">
        <PlusIcon size={14} className="add-plus" />
        <input
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
          placeholder="Link another pack repo: owner/repo or a github.com URL"
          aria-label="GitHub repo to add"
          aria-invalid={!!error}
          spellCheck={false}
        />
        <button className="btn btn-secondary" type="submit" disabled={!!adding}>
          {adding === value.trim() && adding ? "Checking…" : "Add repo"}
        </button>
      </div>
      {error && <p className="field-error">{error}</p>}

      {catalog.length > 0 && (
        <div className="suggest">
          {suggestions.length > 0 && <span className="suggest-label">Suggested:</span>}
          {suggestions.map((s) => (
            <span key={s.repo} className="suggest-chip" title={`${s.description} Updated ${dayAgo(s.updated)}.`}>
              <button type="button" className="suggest-add" onClick={() => void add(s.repo)} disabled={!!adding}>
                {adding === s.repo ? "Adding…" : s.repo}
              </button>
              <button
                type="button"
                className="suggest-view"
                onClick={() => onOpenGithub(`https://github.com/${s.repo}`)}
                aria-label={`View ${s.repo} on GitHub`}
                title="View on GitHub"
              >
                <ExternalIcon size={12} />
              </button>
            </span>
          ))}
          <button type="button" className="suggest-more" onClick={onBrowse}>
            Browse all {catalog.length} configs…
          </button>
        </div>
      )}
    </form>
  );
}
