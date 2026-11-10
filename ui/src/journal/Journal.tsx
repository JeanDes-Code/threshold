import { useState, type CSSProperties, type ReactNode } from "react";
import type { ClosedItem, FactLine, JournalView, NameRef } from "../api";
import { nameText, npcColor } from "../play/logic";
import { bySubject, clockSpan, decisions, placeTitle, sceneOfTurn, scenesNewestFirst } from "./logic";

export type JournalPart = "timeline" | "decisions" | "open" | "before" | "notes";

export const PARTS: { id: JournalPart; label: string }[] = [
  { id: "timeline", label: "Timeline" },
  { id: "decisions", label: "Decisions" },
  { id: "open", label: "Open and closed" },
  { id: "before", label: "Known before the Campaign" },
  { id: "notes", label: "Notes" },
];

interface Props {
  view: JournalView;
  /** The PC's name, for the title. */
  pcName: string;
  /** The part shown first. */
  initialPart?: JournalPart;
}

/**
 * The Journal: the app's record of the Campaign, from PC Knowledge only.
 * One entry per Scene, the decisions, the Open Items, and what the PC knew
 * before the Campaign. It reads the core's JournalView and parses no text.
 */
export default function Journal({ view, pcName, initialPart = "timeline" }: Props) {
  const [part, setPart] = useState<JournalPart>(initialPart);
  const count: Record<JournalPart, number> = {
    timeline: view.scenes.length,
    decisions: decisions(view).length,
    open: view.open.length,
    before: view.known_before.length,
    notes: view.notes.length,
  };

  return (
    <div className="journal">
      <header className="journal-top">
        <p className="play-label">Journal</p>
        <h1 className="journal-top__title">{pcName}</h1>
      </header>
      <div className="journal__body">
        <nav className="journal-nav" aria-label="Journal parts">
          {PARTS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className="journal-nav__item"
              aria-current={part === id ? "page" : undefined}
              onClick={() => setPart(id)}
            >
              <span>{label}</span>
              <span className="journal-nav__count">{count[id]}</span>
            </button>
          ))}
        </nav>
        <main className="journal-page" aria-label={PARTS.find((p) => p.id === part)?.label}>
          <div className="journal-page__measure">
            {part === "timeline" && <Timeline view={view} />}
            {part === "decisions" && <Decisions view={view} />}
            {part === "open" && <OpenAndClosed view={view} />}
            {part === "before" && <KnownBefore lines={view.known_before} />}
            {part === "notes" && <Notes />}
          </div>
        </main>
      </div>
    </div>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <p className="journal-empty">{children}</p>;
}

function Timeline({ view }: { view: JournalView }) {
  const scenes = scenesNewestFirst(view);
  if (scenes.length === 0) return <Empty>The Journal starts when the story begins.</Empty>;
  return (
    <ol className="journal-timeline">
      {scenes.map(({ scene, number }) => (
        <li key={scene.turn_id} className="journal-scene">
          <span className="journal-scene__dot" aria-hidden="true" />
          <article aria-label={`Scene ${number}`}>
            <p className="play-label">
              Scene {number} · {clockSpan(scene.clock_from, scene.clock_to)}
            </p>
            <h2 className="journal-scene__place">{placeTitle(scene.place)}</h2>
            {scene.present.length > 0 && (
              <p className="journal-with">
                <span className="journal-with__label">With</span>
                {scene.present.map((who) => (
                  <Who key={who.entity_id} who={who} />
                ))}
              </p>
            )}
            {scene.acts.length > 0 && (
              <section className="journal-part">
                <h3 className="play-label">What you did</h3>
                <ol className="journal-acts">
                  {scene.acts.map((act) => (
                    <li key={act.turn_id} className="journal-act">
                      <span className="journal-act__mark" aria-hidden="true">
                        ›
                      </span>
                      <span className="journal-act__text">
                        {act.intent}
                        {act.decision && <span className="journal-badge">Decision</span>}
                      </span>
                    </li>
                  ))}
                </ol>
              </section>
            )}
            {scene.learned.length > 0 && (
              <section className="journal-part">
                <h3 className="play-label">What you learned</h3>
                <Facts lines={scene.learned} />
              </section>
            )}
          </article>
        </li>
      ))}
    </ol>
  );
}

function Decisions({ view }: { view: JournalView }) {
  const list = decisions(view);
  if (list.length === 0) return <Empty>No decision yet. A decision is an act that lasts, seen by you.</Empty>;
  return (
    <ol className="journal-decisions">
      {list.map(({ act, scene }) => (
        <li key={act.turn_id} className="journal-decision">
          <p className="journal-decision__intent">{act.intent}</p>
          <p className="journal-meta">
            {placeTitle(scene.place)} · {clockSpan(scene.clock_from, scene.clock_to)}
          </p>
        </li>
      ))}
    </ol>
  );
}

function OpenAndClosed({ view }: { view: JournalView }) {
  return (
    <>
      <section className="journal-section">
        <h2 className="journal-section__title">Open</h2>
        <p className="journal-section__hint">Plans and pending matters you know of. Not done yet.</p>
        {view.open.length === 0 ? <Empty>Nothing open.</Empty> : <Facts lines={view.open} markOpen={false} />}
      </section>
      <section className="journal-section">
        <h2 className="journal-section__title">Closed</h2>
        {view.closed.length === 0 ? (
          <Empty>Nothing closed yet.</Empty>
        ) : (
          <ul className="journal-facts">
            {view.closed.map((closed) => (
              <Closed key={`${closed.item.belief.subject_id}:${closed.item.belief.key}`} view={view} closed={closed} />
            ))}
          </ul>
        )}
      </section>
    </>
  );
}

function Closed({ view, closed }: { view: JournalView; closed: ClosedItem }) {
  const where = sceneOfTurn(view, closed.closed_in);
  return (
    <li className="journal-fact journal-fact--closed">
      <span className="journal-fact__check" aria-hidden="true" />
      <span>
        <span className="journal-fact__text">{closed.item.text}</span>
        {where && (
          <span className="journal-meta journal-meta--block">
            Closed in Scene {where.number} · {placeTitle(where.scene.place)}
          </span>
        )}
      </span>
    </li>
  );
}

function KnownBefore({ lines }: { lines: FactLine[] }) {
  if (lines.length === 0) return <Empty>Nothing was known before the Campaign.</Empty>;
  return (
    <>
      <p className="journal-section__hint">What you knew when the Campaign began.</p>
      {bySubject(lines).map(({ subject, lines: facts }) => (
        <section key={subject.entity_id} className="journal-section journal-subject">
          <h2 className="journal-subject__name">
            <Who who={subject} />
          </h2>
          <Facts lines={facts} />
        </section>
      ))}
    </>
  );
}

function Notes() {
  return <Empty>No notes yet. Your notes about a Scene, a person or a place will show here.</Empty>;
}

/** A name in the Journal: the NPC's colour, or "someone you cannot name". */
function Who({ who }: { who: NameRef }) {
  const style = who.kind === "npc" && who.name !== null ? ({ "--who": npcColor(who.entity_id) } as CSSProperties) : undefined;
  return (
    <span className={who.name === null ? "journal-who journal-who--unknown" : "journal-who"} style={style}>
      {who.kind === "npc" || who.kind === "pc" ? (
        <span className="play-initial journal-who__initial" aria-hidden="true">
          {who.name === null ? "?" : who.name.charAt(0).toUpperCase()}
        </span>
      ) : (
        <span className="play-initial play-initial--thing journal-who__initial" aria-hidden="true" />
      )}
      {who.name === null ? "someone you cannot name" : nameText(who)}
    </span>
  );
}

/** Fact lines: an Open Item gets a ring and, unless `markOpen` is false, a badge; a told fact names who told it. */
function Facts({ lines, markOpen = true }: { lines: FactLine[]; markOpen?: boolean }) {
  return (
    <ul className="journal-facts">
      {lines.map((line) => (
        <li key={`${line.belief.subject_id}:${line.belief.key}`} className="journal-fact">
          <span className={line.open ? "play-fact__mark play-fact__mark--open" : "play-fact__mark"} aria-hidden="true" />
          <span>
            <span className="journal-fact__text">{line.text}</span>
            {line.open &&
              (markOpen ? (
                <span className="journal-badge journal-badge--open">Open</span>
              ) : (
                <span className="play-sr"> (open)</span>
              ))}
            {line.told_by && <span className="journal-meta"> · told by {nameText(line.told_by)}</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}
