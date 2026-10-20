import { useEffect, useRef, useState } from "react";
import { HelpIcon } from "./icons";

interface AccountHelpProps {
  onOpenUserdata: () => void;
}

/** (?) button with a short guide to finding the Steam account folder. */
export function AccountHelp({ onOpenUserdata }: AccountHelpProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <span className="help" ref={ref}>
      <button className="icon-btn icon-btn-sm" aria-label="How to find your Steam account" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <HelpIcon size={15} />
      </button>
      {open && (
        <div className="popover" role="dialog" aria-label="Finding your Steam account">
          <h3>Which account is mine?</h3>
          <ol>
            <li>
              Each Steam account has a numbered folder in <code>Steam\userdata</code>.
            </li>
            <li>
              That number is your <strong>Steam friend code</strong> (Steam → Friends → Add a friend shows it).
            </li>
            <li>
              Deadlock keeps its <code>video.txt</code> in <code>userdata\&lt;number&gt;\1422450\local\cfg</code>. It only exists
              after you've launched the game once on that account.
            </li>
          </ol>
          <button className="btn btn-secondary btn-sm" onClick={onOpenUserdata}>
            Open userdata folder
          </button>
        </div>
      )}
    </span>
  );
}
