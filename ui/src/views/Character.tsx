import { useEffect, useState } from "react";
import { clockText, errorText, type CharacterView } from "../api";
import { api } from "../tauri";

/** The PC as the PC knows itself. `revision` changes after each Turn. */
export default function Character({ revision }: { revision: number }) {
  const [view, setView] = useState<CharacterView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getCharacter().then(setView, (e: unknown) => setError(errorText(e)));
  }, [revision]);

  if (error) return <p className="error">{error}</p>;
  if (!view) return <p className="muted">Loading…</p>;
  return (
    <section>
      <h2>{view.name}</h2>
      <p>{view.concept}</p>
      <p className="muted">{clockText(view.clock_min)}</p>
      <h2>What {view.name} knows about themself</h2>
      {view.facts.length === 0 ? (
        <p className="muted">Nothing yet.</p>
      ) : (
        <ul>
          {view.facts.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>
      )}
    </section>
  );
}
