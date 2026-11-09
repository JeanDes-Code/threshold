import { useEffect, useState } from "react";
import { errorText, type DebugView, type TurnLine } from "../api";
import { api } from "../tauri";

/** Prompts, raw model output and timings of one Turn. Debug mode only. */
export default function Debug({ lines }: { lines: TurnLine[] }) {
  const [turnId, setTurnId] = useState<number | null>(lines.at(-1)?.turn_id ?? null);
  const [view, setView] = useState<DebugView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (turnId === null) return;
    api.getDebug(turnId).then(
      (v) => {
        setView(v);
        setError(null);
      },
      (e: unknown) => setError(errorText(e)),
    );
  }, [turnId]);

  if (lines.length === 0) return <p className="muted">No Turn yet.</p>;
  return (
    <section>
      <label htmlFor="turn">Turn</label>
      <select
        id="turn"
        value={turnId ?? ""}
        onChange={(e) => setTurnId(Number(e.target.value))}
      >
        {lines.map((l) => (
          <option key={l.turn_id} value={l.turn_id}>
            {l.turn_id}: {l.intent.slice(0, 60)}
          </option>
        ))}
      </select>
      {error && <p className="error">{error}</p>}
      {view && (
        <>
          <p className="muted">
            op_id {view.op_id}, kind {view.kind}
          </p>
          <h2>Stored proposal</h2>
          <pre>{view.proposal_json}</pre>
          <h2>Steps</h2>
          {view.entries.map((e, i) => (
            <details key={i}>
              <summary>
                {e.step} ({e.ms} ms)
              </summary>
              {e.prompt && (
                <>
                  <label>Prompt</label>
                  <pre>{e.prompt}</pre>
                </>
              )}
              <label>Output</label>
              <pre>{e.raw}</pre>
            </details>
          ))}
        </>
      )}
    </section>
  );
}
