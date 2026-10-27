import { errorText } from "../api";

interface LaunchOptionsProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** The launch options line most players use, plus -condebug so the Performance view gets the game's FPS
 * reports. No -dev: it fills console.log with developer warnings (hundreds of MB per match). */
export const RECOMMENDED = "-novid +cl_forcepreload 1 -noassert -console -noasyncmaterialload -dx11 -condebug";

/** The recommended launch options, ready to copy into Steam. */
export function LaunchOptions({ onNotify }: LaunchOptionsProps) {
  const copy = () =>
    navigator.clipboard.writeText(RECOMMENDED).then(
      () => onNotify("ok", "Copied. Paste it in Steam: right-click Deadlock › Properties › Launch options."),
      (e) => onNotify("error", errorText(e)),
    );

  return (
    <div className="launch-recommended">
      <code>
        {/* One span per option so a line never breaks inside one (e.g. after its dash). */}
        {RECOMMENDED.split(" ").map((o, i) => (
          <span key={i}>{i > 0 && " "}<span className="nowrap">{o}</span></span>
        ))}
      </code>
      <button className="btn btn-secondary btn-sm" onClick={() => void copy()}>
        Copy
      </button>
    </div>
  );
}
