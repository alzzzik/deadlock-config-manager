import { FormEvent, useState } from "react";

/** Well-known pack repos offered as one-click suggestions. They are never linked automatically. */
const POPULAR_REPOS: { repo: string; note: string }[] = [
  { repo: "Sqooky/OptimizationLock", note: "lots of community configs" },
  { repo: "dacooderr/OptiLock", note: "recommended, potato and ultra-potato presets" },
];

interface AddRepoProps {
  /** Repos already linked, so their suggestions are hidden. */
  linked: string[];
  onAdd: (input: string) => Promise<string | null>;
  onOpenGithub: (url: string) => void;
}

/** Inline field for linking another pack repo. `onAdd` resolves to an error message or null. */
export function AddRepo({ linked, onAdd, onOpenGithub }: AddRepoProps) {
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);

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
  const suggestions = POPULAR_REPOS.filter((s) => !isLinked(s.repo));

  return (
    <form className="add-repo" onSubmit={submit}>
      <div className="add-row">
        <span className="add-plus" aria-hidden>
          +
        </span>
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
        <button className="btn" type="submit" disabled={!!adding}>
          {adding === value.trim() && adding ? "Checking…" : "Add repo"}
        </button>
      </div>
      {error && <p className="field-error">{error}</p>}

      {suggestions.length > 0 && (
        <div className="suggest">
          <span className="suggest-label">Popular repos:</span>
          {suggestions.map((s) => (
            <span key={s.repo} className="suggest-chip" title={s.note}>
              <button type="button" className="suggest-add" onClick={() => void add(s.repo)} disabled={!!adding}>
                {adding === s.repo ? "Adding…" : `+ ${s.repo}`}
              </button>
              <button
                type="button"
                className="suggest-view"
                onClick={() => onOpenGithub(`https://github.com/${s.repo}`)}
                aria-label={`View ${s.repo} on GitHub`}
                title="View on GitHub"
              >
                ↗
              </button>
            </span>
          ))}
        </div>
      )}
    </form>
  );
}
