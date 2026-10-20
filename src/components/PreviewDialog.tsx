import { useState } from "react";
import { Change, FileDiff, Preview } from "../api";
import { Modal } from "./Modal";

interface PreviewDialogProps {
  preview: Preview;
  onCancel: () => void;
  onConfirm: () => void;
}

const COLLAPSED_ROWS = 30;

function ChangeRow({ c }: { c: Change }) {
  const kind = c.before === null ? "added" : c.after === null ? "removed" : "changed";
  return (
    <li className={`change change-${kind}`}>
      <code className="change-key" title={c.key}>
        {c.key}
      </code>
      <span className="change-vals">
        {kind === "added" && <span className="change-tag">new</span>}
        {kind !== "added" && <span className="change-before">{c.before}</span>}
        {kind === "changed" && <span className="change-arrow">→</span>}
        {kind !== "removed" ? <span className="change-after">{c.after}</span> : <span className="change-tag">removed</span>}
      </span>
    </li>
  );
}

function FileSection({ d }: { d: FileDiff }) {
  const [all, setAll] = useState(false);
  const shown = all ? d.changes : d.changes.slice(0, COLLAPSED_ROWS);
  const summary = d.created
    ? `new file · ${d.changes.length} settings`
    : d.changes.length === 0
      ? "no changes"
      : `${d.changes.length} change${d.changes.length === 1 ? "" : "s"} · ${d.unchanged} unchanged`;
  return (
    <section className="diff-file">
      <header className="diff-head">
        <code>{d.file}</code>
        <span className="muted small">{summary}</span>
      </header>
      {d.changes.length > 0 && (
        <ul className="change-list">
          {shown.map((c) => (
            <ChangeRow key={c.key} c={c} />
          ))}
        </ul>
      )}
      {d.changes.length > COLLAPSED_ROWS && (
        <button className="btn btn-quiet btn-sm" onClick={() => setAll((v) => !v)}>
          {all ? "Show fewer" : `Show all ${d.changes.length}`}
        </button>
      )}
    </section>
  );
}

export function PreviewDialog({ preview, onCancel, onConfirm }: PreviewDialogProps) {
  const nothing = preview.files.every((f) => !f.created && f.changes.length === 0);
  return (
    <Modal
      title={`Apply ${preview.packName}?`}
      onClose={onCancel}
      wide
      footer={
        <>
          <span className="muted small">A backup is taken first, so you can always restore.</span>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={onConfirm} autoFocus>
            {nothing ? "Apply anyway" : "Apply"}
          </button>
        </>
      }
    >
      {preview.warnings.length > 0 && (
        <ul className="warning-list">
          {preview.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {nothing && <p className="muted">Nothing would change: your files already match this pack.</p>}
      {preview.files.map((d) => (
        <FileSection key={d.file} d={d} />
      ))}
    </Modal>
  );
}
