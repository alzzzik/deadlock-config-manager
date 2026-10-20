import { FormEvent, useState } from "react";

interface AddRepoProps {
  onAdd: (input: string) => Promise<string | null>;
}

/** Inline field for linking another pack repo. `onAdd` resolves to an error message or null. */
export function AddRepo({ onAdd }: AddRepoProps) {
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!value.trim()) {
      setError("Paste a GitHub repo first, like owner/repo.");
      return;
    }
    setAdding(true);
    const err = await onAdd(value.trim());
    setAdding(false);
    setError(err);
    if (!err) setValue("");
  };

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
        <button className="btn" type="submit" disabled={adding}>
          {adding ? "Checking…" : "Add repo"}
        </button>
      </div>
      {error && <p className="field-error">{error}</p>}
    </form>
  );
}
