import { errorText } from "../api";

interface LaunchOptionsProps {
  onNotify: (kind: "ok" | "error" | "info", text: string) => void;
}

/** The launch options line most players use. */
export const RECOMMENDED = "-novid +cl_forcepreload 1 -noassert -dev -console -noasyncmaterialload -dx11";

/** The recommended launch options, ready to copy into Steam. */
export function LaunchOptions({ onNotify }: LaunchOptionsProps) {
  const copy = () =>
    navigator.clipboard.writeText(RECOMMENDED).then(
      () => onNotify("ok", "Copied. Paste it in Steam: right-click Deadlock › Properties › Launch options."),
      (e) => onNotify("error", errorText(e)),
    );

  return (
    <div className="launch-recommended">
      <code>{RECOMMENDED}</code>
      <button className="btn btn-secondary btn-sm" onClick={() => void copy()}>
        Copy
      </button>
    </div>
  );
}
