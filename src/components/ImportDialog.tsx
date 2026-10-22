import { useState } from "react";
import { Modal } from "./Modal";

interface ImportDialogProps {
  busy: boolean;
  onCancel: () => void;
  onCode: (text: string) => void;
  onZip: () => void;
  onFolder: () => void;
}

/** Add a config to My configs from a share code, a zip or a folder. */
export function ImportDialog({ busy, onCancel, onCode, onZip, onFolder }: ImportDialogProps) {
  const [text, setText] = useState("");
  const hasCode = text.includes("DCM1-");
  return (
    <Modal
      title="Import a config"
      onClose={onCancel}
      footer={
        <>
          <button className="btn btn-quiet" onClick={onZip} disabled={busy}>
            Zip file…
          </button>
          <button className="btn btn-quiet" onClick={onFolder} disabled={busy}>
            Folder…
          </button>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => onCode(text)} disabled={busy || !hasCode}>
            Import code
          </button>
        </>
      }
    >
      <p className="muted small">
        Paste a share code (or the whole message someone sent you). It's added to My configs, where you can preview it
        before applying.
      </p>
      <textarea
        className="tweak-lines"
        rows={4}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="DCM1-…"
        spellCheck={false}
        autoFocus
      />
    </Modal>
  );
}
