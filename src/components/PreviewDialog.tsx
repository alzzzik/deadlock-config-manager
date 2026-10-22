import { useState } from "react";
import { Change, ConvarInfo, FileDiff, Preview } from "../api";
import { Modal } from "./Modal";

interface PreviewDialogProps {
  preview: Preview;
  onCancel: () => void;
  onConfirm: () => void;
  onConfirmSafe: () => void;
}

const COLLAPSED_ROWS = 30;

const CONVARS_PREFIX = "ConVars › ";

function ChangeRow({ c, info }: { c: Change; info?: ConvarInfo }) {
  const kind = c.before === null ? "added" : c.after === null ? "removed" : "changed";
  const isConvar = c.key.startsWith(CONVARS_PREFIX);
  const text = info?.text ?? (isConvar && !info?.ignored ? "No description yet." : null);
  return (
    <li className={`change change-${kind} ${info?.ignored ? "is-ignored" : ""}`}>
      <div className="change-label">
        <code className="change-key" title={c.key}>
          {isConvar ? c.key.slice(CONVARS_PREFIX.length) : c.key}
        </code>
        {info?.ignored && (
          <span className="change-tag is-ignored" title="Valve marked this setting gameinfo_cannot_override, so the game skips it in gameinfo.gi.">
            ignored by the game
          </span>
        )}
        {text && <small className="change-note">{text}</small>}
      </div>
      <span className="change-vals">
        {kind === "added" && <span className="change-tag">new</span>}
        {kind !== "added" && <span className="change-before">{c.before}</span>}
        {kind === "changed" && <span className="change-arrow">→</span>}
        {kind !== "removed" ? <span className="change-after">{c.after}</span> : <span className="change-tag">removed</span>}
      </span>
    </li>
  );
}

function FileSection({ d, convars }: { d: FileDiff; convars: Record<string, ConvarInfo> }) {
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
            <ChangeRow key={c.key} c={c} info={convars[c.key]} />
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

function RankedNotice({ preview }: { preview: Preview }) {
  const [open, setOpen] = useState(false);
  const n = preview.rankedChanges.length;
  if (preview.rankedSafe) {
    return (
      <p className="notice is-good">
        Ranked-safe: the engine sections Valve checks before matchmaking are set to Valve's defaults. The pack's other
        changes are kept.
      </p>
    );
  }
  if (n === 0) return null;
  return (
    <div className="ranked-notice">
      <p>
        <strong>May block ranked matchmaking.</strong> This pack changes {n} engine setting{n === 1 ? "" : "s"} that
        Valve checks before ranked queueing. <b>Apply ranked-safe</b> keeps everything else and puts those sections back
        to Valve's defaults.
      </p>
      <button className="btn btn-quiet btn-sm" onClick={() => setOpen((v) => !v)} aria-expanded={open}>
        {open ? "Hide engine settings" : "Show engine settings"}
      </button>
      {open && (
        <ul className="change-list">
          {preview.rankedChanges.map((c) => (
            <ChangeRow key={c.key} c={c} />
          ))}
        </ul>
      )}
    </div>
  );
}

export function PreviewDialog({ preview, onCancel, onConfirm, onConfirmSafe }: PreviewDialogProps) {
  const nothing = preview.files.every((f) => !f.created && f.changes.length === 0);
  const offerSafe = !preview.rankedSafe && preview.rankedChanges.length > 0;
  const ignored = Object.values(preview.convars).filter((i) => i.ignored).length;
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
          {offerSafe ? (
            <>
              <button className="btn btn-secondary" onClick={onConfirm}>
                Apply as is
              </button>
              <button className="btn btn-primary" onClick={onConfirmSafe} autoFocus>
                Apply ranked-safe
              </button>
            </>
          ) : (
            <button className="btn btn-primary" onClick={onConfirm} autoFocus>
              {nothing ? "Apply anyway" : "Apply"}
            </button>
          )}
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
      <RankedNotice preview={preview} />
      {preview.tweaks > 0 && (
        <p className="muted small">
          Includes your {preview.tweaks} tweak{preview.tweaks === 1 ? "" : "s"} on top of the pack.
        </p>
      )}
      {nothing && <p className="muted">Nothing would change: your files already match this pack.</p>}
      {ignored > 0 && (
        <p className="muted small">
          {ignored} setting{ignored === 1 ? "" : "s"} in this pack {ignored === 1 ? "is" : "are"} ignored by the game: Valve
          locked {ignored === 1 ? "it" : "them"} so gameinfo.gi can't change {ignored === 1 ? "it" : "them"}. They're
          marked below. Descriptions are from game build {preview.convarBuild}.
        </p>
      )}
      {preview.files.map((d) => (
        <FileSection key={d.file} d={d} convars={preview.convars} />
      ))}
    </Modal>
  );
}
