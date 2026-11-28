import { useEffect, useState } from "react";
import { Modal } from "./Modal";
import { LaunchOptions } from "./LaunchOptions";
import { SetupHints, api } from "../api";

/** A starting config, found by repo and folder once the repo is linked. */
export interface GuideChoice {
  id: string;
  title: string;
  detail: string;
  repo: string;
  folder: string;
}

const ALZ_REPO = "alzzzik/alz-deadlock-cfg";
const OPTILOCK_REPO = "dacooderr/OptiLock";

export const GUIDE_CHOICES: GuideChoice[] = [
  {
    id: "most",
    title: "Most PCs",
    detail: "OptiLock FPS Config. More FPS for competitive play on low and mid-range PCs, with a video.txt.",
    repo: OPTILOCK_REPO,
    folder: "OptiLock FPS Config (Recommended)",
  },
  {
    id: "low",
    title: "Low-end PC",
    detail: "OptiLock Potato. Turns most effects down for the highest FPS on weak hardware.",
    repo: OPTILOCK_REPO,
    folder: "OptiLock Potato Config",
  },
  {
    id: "mods",
    title: "stable",
    detail: "alz stable. More FPS on any GPU, made to run alongside your mods (skins, HUD and others). No upscaler in video.txt.",
    repo: ALZ_REPO,
    folder: "stable",
  },
  {
    id: "complock",
    title: "apex",
    detail: "alz apex. The best 1% lows; recommended to run alongside CompLock's addons (HUD++, Particle Pruning, Sound cap). NVIDIA RTX, DLSS 1.7x in video.txt.",
    repo: ALZ_REPO,
    folder: "apex-complock",
  },
];

/** The choice to start on, and why, from what's installed on this PC. */
export function suggestChoice(hints: SetupHints | null): { id: string; reason: string | null } {
  if (hints?.complock && hints.nvidia) return { id: "complock", reason: "CompLock's mod paks and an NVIDIA GPU were found." };
  if (hints?.mods) return { id: "mods", reason: "Mods were found in your game folder." };
  return { id: "most", reason: null };
}

interface FirstRunGuideProps {
  busy: boolean;
  onPick: (choice: GuideChoice) => void;
  onSkip: () => void;
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** First launch: pick a starting config instead of facing a list of repos. */
export function FirstRunGuide({ busy, onPick, onSkip, onNotify }: FirstRunGuideProps) {
  const [hints, setHints] = useState<SetupHints | null>(null);
  const [picked, setPicked] = useState<GuideChoice>(GUIDE_CHOICES[0]);
  const [touched, setTouched] = useState(false);

  useEffect(() => {
    let live = true;
    api.setupHints().then(
      (h) => live && setHints(h),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);

  const suggestion = suggestChoice(hints);
  // Move to the suggestion once the hints arrive, unless the player already picked something.
  useEffect(() => {
    if (touched) return;
    const c = GUIDE_CHOICES.find((x) => x.id === suggestion.id);
    if (c) setPicked(c);
  }, [suggestion.id, touched]);

  const pick = (c: GuideChoice) => {
    setTouched(true);
    setPicked(c);
  };
  const complockMissing = picked.id === "complock" && hints !== null && !hints.complock;

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
        {suggestion.reason && ` ${suggestion.reason}`}
      </p>
      <div className="guide-choices" role="radiogroup" aria-label="Starting config">
        {GUIDE_CHOICES.map((c) => (
          <label key={c.id} className={`guide-choice ${picked.id === c.id ? "is-picked" : ""}`}>
            <input type="radio" name="guide" checked={picked.id === c.id} onChange={() => pick(c)} />
            <span>
              <strong>
                {c.title}
                {suggestion.reason && suggestion.id === c.id && <span className="badge is-accent guide-suggested">Suggested</span>}
              </strong>
              <small className="muted">{c.detail}</small>
            </span>
          </label>
        ))}
      </div>
      {complockMissing && (
        <p className="notice is-warn small">
          CompLock's addons weren't found in your game folder. apex gives lower 1% lows without them; pick stable instead or install them first.
        </p>
      )}
      <p className="muted small guide-launch">Also add these launch options in Steam (right-click Deadlock › Properties):</p>
      <LaunchOptions onNotify={onNotify} />
    </Modal>
  );
}
