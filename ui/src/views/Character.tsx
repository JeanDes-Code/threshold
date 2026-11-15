import { useEffect, useState } from "react";
import { clockText, errorText, type CharacterView, type NameRef } from "../api";
import { nameText } from "../play/logic";
import { api } from "../tauri";

/** The last Character view read, per Campaign file: shown at once when the
 * tab opens again, even while a Turn runs and `get_character` waits for it
 * (QA m4, 2026-11-15: the tab showed a bare "Loading…" for the whole Turn). */
let lastRead: { campaign: string; view: CharacterView } | null = null;

/** The line above a Character view that a running Turn will change. */
export const CHARACTER_RUNNING_LINE = "The GM is playing a Turn. This shows you as of the last Turn.";

interface Props {
  /** Changes after each Turn: the view is read again. */
  revision: number;
  /** The open Campaign's file: the last view read is kept per Campaign. */
  campaign: string;
  /** A Turn is running: the tab keeps the last view read until it ends. */
  running: boolean;
}

/**
 * The PC as the PC knows itself, in the cards of the Play screen: who the
 * PC is, the state the core holds, what the PC knows about itself, and its
 * open items. The text comes from the view as it is. `revision` changes
 * after each Turn.
 */
export default function Character({ revision, campaign, running }: Props) {
  const [view, setView] = useState<CharacterView | null>(() =>
    lastRead?.campaign === campaign ? lastRead.view : null,
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getCharacter().then(
      (read) => {
        lastRead = { campaign, view: read };
        setView(read);
      },
      (e: unknown) => setError(errorText(e)),
    );
  }, [revision, campaign]);

  if (error) return <p className="error">{error}</p>;
  if (!view) return <p className="muted">{running ? CHARACTER_RUNNING_LINE : "Loading…"}</p>;
  return (
    <div className="character">
      {running && <p className="muted character__running">{CHARACTER_RUNNING_LINE}</p>}
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
        <Facts lines={view.facts.map((f) => ({ ...f, open: false }))} empty="Nothing yet." />
      </section>

      <section className="play-card" aria-label="Open">
        <h2 className="play-label">Open</h2>
        <Facts lines={view.open} empty="Nothing open." />
      </section>
    </div>
  );
}

function Facts({
  lines,
  empty,
}: {
  lines: { text: string; open: boolean; told_by: NameRef | null }[];
  empty: string;
}) {
  if (lines.length === 0) return <p className="play-empty">{empty}</p>;
  return (
    <ul className="play-list">
      {lines.map((f, i) => (
        <li key={i} className="play-fact">
          <span className={f.open ? "play-fact__mark play-fact__mark--open" : "play-fact__mark"} aria-hidden="true" />
          <span>
            {f.text}
            {f.open && <span className="play-sr"> (open: planned or pending, not done yet)</span>}
            {f.told_by && <span className="play-faint"> · told by {nameText(f.told_by)}</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}
