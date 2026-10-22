import { useState } from "react";
import { LibraryEntry, timeAgo } from "../api";
import { Modal } from "./Modal";
import { GroupHead, RepoMenu } from "./RepoSection";
import { FolderIcon } from "./icons";

interface MyConfigsProps {
  entries: LibraryEntry[];
  /** `lib:<id>` of the applied saved config, if any. */
  appliedId: string | null;
  disabled: boolean;
  canSave: boolean;
  onApply: (e: LibraryEntry) => void;
  onSave: () => void;
  onImport: () => void;
  onExport: (e: LibraryEntry) => void;
  onShare: (e: LibraryEntry) => void;
  onDelete: (e: LibraryEntry) => void;
}

/** Saved configs that don't come from a repo. */
export function MyConfigs({ entries, appliedId, disabled, canSave, onApply, onSave, onImport, onExport, onShare, onDelete }: MyConfigsProps) {
  return (
    <section className="group" aria-labelledby="my-configs-title">
      <GroupHead
        icon={<FolderIcon size={14} />}
        title={<span id="my-configs-title">My configs</span>}
        meta={entries.length === 0 ? "Save what's installed, or import a zip" : `${entries.length} saved`}
      >
        <button className="btn btn-quiet btn-sm" onClick={onImport}>
          Import…
        </button>
        <button className="btn btn-quiet btn-sm" onClick={onSave} disabled={!canSave} title="Save the gameinfo.gi and video.txt installed now">
          Save current…
        </button>
      </GroupHead>
      {entries.length > 0 && (
        <ul className="pack-list">
          {entries.map((e) => {
            const applied = appliedId === `lib:${e.id}`;
            return (
              <li key={e.id} className={`pack-row ${applied ? "is-applied" : ""}`}>
                <span className="row-pin" aria-hidden />
                <div className="pack-main">
                  <div className="pack-title">
                    <h3>{e.name}</h3>
                    {applied && <span className="badge is-good">Applied</span>}
                  </div>
                  <p className="pack-desc" title={e.notes || e.source}>
                    {e.notes || e.source || "Saved config"}
                  </p>
                </div>
                <div className="pack-meta">
                  <span>Saved {timeAgo(e.createdAt)}</span>
                  {!e.hasVideo && <span className="pack-commit">gameinfo.gi only</span>}
                </div>
                <div className="pack-actions">
                  <RepoMenu
                    label={`Options for ${e.name}`}
                    items={[
                      { label: "Copy share code", onClick: () => onShare(e) },
                      { label: "Export as zip…", onClick: () => onExport(e) },
                      { label: "Delete", onClick: () => onDelete(e), danger: true },
                    ]}
                  />
                  <button className="btn btn-secondary btn-sm" disabled={disabled} onClick={() => onApply(e)}>
                    {applied ? "Re-apply" : "Apply"}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

interface SaveConfigDialogProps {
  suggestedName: string;
  onCancel: () => void;
  onSave: (name: string, notes: string) => void;
}

export function SaveConfigDialog({ suggestedName, onCancel, onSave }: SaveConfigDialogProps) {
  const [name, setName] = useState(suggestedName);
  const [notes, setNotes] = useState("");
  return (
    <Modal
      title="Save current config"
      onClose={onCancel}
      footer={
        <>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => onSave(name, notes)} disabled={!name.trim()}>
            Save
          </button>
        </>
      }
    >
      <p className="muted small">
        Saves the gameinfo.gi and video.txt installed now, without your GPU lines, so you can apply them again later or
        share them.
      </p>
      <label className="text-field">
        <span>Name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} maxLength={60} autoFocus />
      </label>
      <label className="text-field">
        <span>Notes (optional)</span>
        <input value={notes} onChange={(e) => setNotes(e.target.value)} maxLength={500} placeholder="e.g. Stable with FOV 100" />
      </label>
    </Modal>
  );
}
