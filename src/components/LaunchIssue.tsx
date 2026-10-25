import { LaunchFailure } from "../api";

interface LaunchIssueProps {
  issue: LaunchFailure;
  busy: boolean;
  onRestore: () => void;
  onKeep: () => void;
}

/** Deadlock closed right after starting following the app's last change: offer the files from before it. */
export function LaunchIssue({ issue, busy, onRestore, onKeep }: LaunchIssueProps) {
  return (
    <div className="notice is-warn notice-action launch-issue" role="alert">
      <span>
        Deadlock closed {issue.ranSeconds}s after starting, after {issue.config} was applied. This can happen when a config
        doesn't fit the current game version.
        {issue.fatal && <code className="launch-fatal">{issue.fatal}</code>}
      </span>
      <span className="launch-actions">
        <button className="btn btn-secondary btn-sm" disabled={busy} onClick={onRestore}>
          Restore previous files
        </button>
        <button className="btn btn-quiet btn-sm" disabled={busy} onClick={onKeep}>
          Keep
        </button>
      </span>
    </div>
  );
}
