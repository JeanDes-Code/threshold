import { useEffect, useState } from "react";
import { clockText, errorText, type CharacterView } from "../api";
import { api } from "../tauri";

/**
 * The PC as the PC knows itself, in the cards of the Play screen: who the
 * PC is, the state the core holds, what the PC knows about itself, and its
 * open items. The text comes from the view as it is. `revision` changes
 * after each Turn.
 */
export default function Character({ revision }: { revision: number }) {
  const [view, setView] = useState<CharacterView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getCharacter().then(setView, (e: unknown) => setError(errorText(e)));
  }, [revision]);

  if (error) return <p className="error">{error}</p>;
  if (!view) return <p className="play-empty">Loading…</p>;
  return (
    <div className="character">
      <section className="play-card" aria-label="You">
        <h2 className="play-label">You</h2>
        <p className="play-you__name">{view.name}</p>
        {view.concept !== "" && <p className="play-you__concept">{view.concept}</p>}
        <p className="play-faint character__clock">{clockText(view.clock_min)}</p>
      </section>

      <section className="play-card" aria-label="State">
        <h2 className="play-label">State</h2>
        {view.state.length === 0 ? (
          <p className="play-empty">Nothing to note.</p>
        ) : (
          <ul className="play-list play-you__state character__state">
            {view.state.map((line) => (
              <li key={line.key} className="play-you__row">
                {line.text}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="play-card" aria-label="What you know about yourself">
        <h2 className="play-label">What you know about yourself</h2>
        <Facts lines={view.facts.map((f) => ({ text: f.text, open: false }))} empty="Nothing yet." />
      </section>

      <section className="play-card" aria-label="Open">
        <h2 className="play-label">Open</h2>
        <Facts lines={view.open} empty="Nothing open." />
      </section>
    </div>
  );
}

function Facts({ lines, empty }: { lines: { text: string; open: boolean }[]; empty: string }) {
  if (lines.length === 0) return <p className="play-empty">{empty}</p>;
  return (
    <ul className="play-list">
      {lines.map((f, i) => (
        <li key={i} className="play-fact">
          <span className={f.open ? "play-fact__mark play-fact__mark--open" : "play-fact__mark"} aria-hidden="true" />
          <span>
            {f.text}
            {f.open && <span className="play-sr"> (open: planned or pending, not done yet)</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}
