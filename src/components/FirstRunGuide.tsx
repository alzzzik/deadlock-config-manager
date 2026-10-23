import { useState } from "react";
import { Modal } from "./Modal";

/** A starting config, found by repo and folder once the repo is linked. */
export interface GuideChoice {
  id: string;
  title: string;
  detail: string;
  repo: string;
  folder: string;
}

export const GUIDE_CHOICES: GuideChoice[] = [
  {
    id: "most",
    title: "Most PCs",
    detail: "alz Stable. More FPS, works with mods, no upscaler, so it suits any GPU.",
    repo: "alzzzik/alz-deadlock-cfg",
    folder: "stable",
  },
  {
    id: "rtx",
    title: "NVIDIA RTX",
    detail: "alz's own setup: the same gameinfo.gi as Stable with DLSS at 1.7x in video.txt.",
    repo: "alzzzik/alz-deadlock-cfg",
    folder: "alz",
  },
  {
    id: "low",
    title: "Low-end PC",
    detail: "OptiLock Potato. Turns most effects down for the highest FPS on weak hardware.",
    repo: "dacooderr/OptiLock",
    folder: "OptiLock Potato Config",
  },
];

interface FirstRunGuideProps {
  busy: boolean;
  onPick: (choice: GuideChoice) => void;
  onSkip: () => void;
}

/** First launch: pick a starting config instead of facing a list of repos. */
export function FirstRunGuide({ busy, onPick, onSkip }: FirstRunGuideProps) {
  const [picked, setPicked] = useState<GuideChoice>(GUIDE_CHOICES[0]);
  return (
    <Modal
      title="Pick a starting config"
      onClose={onSkip}
      footer={
        <>
          <button className="btn btn-quiet" onClick={onSkip}>
            Skip, I'll choose myself
          </button>
          <span className="spacer" />
          <button className="btn btn-primary" onClick={() => onPick(picked)} disabled={busy} autoFocus>
            Preview changes
          </button>
        </>
      }
    >
      <p className="muted small">
        You'll see every change before anything is written, and your current files are backed up first.
      </p>
      <div className="guide-choices" role="radiogroup" aria-label="Starting config">
        {GUIDE_CHOICES.map((c) => (
          <label key={c.id} className={`guide-choice ${picked.id === c.id ? "is-picked" : ""}`}>
            <input type="radio" name="guide" checked={picked.id === c.id} onChange={() => setPicked(c)} />
            <span>
              <strong>{c.title}</strong>
              <small className="muted">{c.detail}</small>
            </span>
          </label>
        ))}
      </div>
    </Modal>
  );
}
