import { useEffect, useState } from "react";
import { BackupMeta, api, errorText, formatBackupId } from "../api";
import { Modal } from "./Modal";

interface RestoreDialogProps {
  busy: boolean;
  onClose: () => void;
  onRestore: (b: BackupMeta) => void;
  onOpenFolder: () => void;
}

function contents(b: BackupMeta): string {
  const files = [b.gameinfoPath && "gameinfo.gi", b.videoPath && "video.txt"].filter(Boolean);
  return files.length ? files.join(" + ") : "empty";
}

export function RestoreDialog({ busy, onClose, onRestore, onOpenFolder }: RestoreDialogProps) {
  const [backups, setBackups] = useState<BackupMeta[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listBackups().then(setBackups, (e) => setError(errorText(e)));
  }, []);

  const original = backups?.find((b) => b.original);
  const others = backups?.filter((b) => !b.original) ?? [];

  return (
    <Modal
      title="Restore"
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-ghost" onClick={onOpenFolder}>
            Open backups folder
          </button>
          <span className="spacer" />
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      {error && <p className="banner banner-bad">{error}</p>}
      {backups && backups.length === 0 && (
        <p className="muted">No backups yet. One is taken automatically before every apply.</p>
      )}

      {original && (
        <div className="restore-original">
          <div>
            <h3>Original files</h3>
            <p className="muted small">
              Taken {formatBackupId(original.id)} before the first apply · {contents(original)}
            </p>
          </div>
          <button className="btn btn-primary" disabled={busy} onClick={() => onRestore(original)}>
            Restore original
          </button>
        </div>
      )}

      {others.length > 0 && (
        <ul className="backup-list">
          {others.map((b) => (
            <li key={b.id}>
              <div>
                <strong>{formatBackupId(b.id)}</strong>
                <span className="muted small">
                  {b.reason} · {contents(b)}
                </span>
              </div>
              <button className="btn btn-ghost" disabled={busy} onClick={() => onRestore(b)}>
                Restore
              </button>
            </li>
          ))}
        </ul>
      )}

      <p className="hint">
        Need Valve's stock gameinfo.gi? In Steam: Deadlock → Properties → Installed Files → <em>Verify integrity of game
        files</em>.
      </p>
    </Modal>
  );
}
