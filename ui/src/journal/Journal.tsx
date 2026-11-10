import { useState, type CSSProperties, type ReactNode } from "react";
import type { ClosedItem, FactLine, JournalView, NameRef, Note, Target } from "../api";
import { factTarget, isPinned, notesAbout, notesNewestFirst } from "../marks/logic";
import NoteField from "../marks/NoteField";
import PinButton from "../marks/PinButton";
import { nameText, npcColor } from "../play/logic";
import {
  aboutText,
  bySubject,
  clockSpan,
  decisions,
  placeTitle,
  sceneOfTurn,
  scenesNewestFirst,
} from "./logic";

export type JournalPart = "timeline" | "decisions" | "open" | "before" | "notes";

export const PARTS: { id: JournalPart; label: string }[] = [
  { id: "timeline", label: "Timeline" },
  { id: "decisions", label: "Decisions" },
  { id: "open", label: "Open and closed" },
  { id: "before", label: "Known before the Campaign" },
  { id: "notes", label: "Notes" },
];

/**
 * What the Player can do in the Journal: pin, and write Notes. Each one
 * resolves with the core's refusal text, or null when it worked.
 */
export interface JournalActions {
  onPin: (target: Target, on: boolean) => void;
  onAddNote: (body: string, about: Target | null) => Promise<string | null>;
  onEditNote: (noteId: number, body: string) => Promise<string | null>;
  onDeleteNote: (noteId: number) => Promise<string | null>;
}

interface Props {
  view: JournalView;
  /** The PC's name, for the title. */
  pcName: string;
  actions: JournalActions;
  /** The part shown first. */
  initialPart?: JournalPart;
  /** The screen switch and the Campaign menu, on the right of the top bar. */
  tools?: ReactNode;
}

/**
 * The Journal: the app's record of the Campaign, from PC Knowledge only.
 * One entry per Scene, the decisions, the Open Items, what the PC knew
 * before the Campaign, and the Player's own Notes and Pins. It reads the
 * core's JournalView and parses no text.
 */
export default function Journal({ view, pcName, actions, initialPart = "timeline", tools }: Props) {
  const [part, setPart] = useState<JournalPart>(initialPart);
  const count: Record<JournalPart, number> = {
    timeline: view.scenes.length,
    decisions: decisions(view).length,
    open: view.open.length,
    before: view.known_before.length,
    notes: view.notes.length,
  };
  const pin = (target: Target, label: string) => (
    <PinButton pinned={isPinned(view.pins, target)} label={label} onToggle={(on) => actions.onPin(target, on)} />
  );

  return (
    <div className="journal">
      <header className="journal-top">
        <div>
          <p className="play-label">Journal</p>
          <h1 className="journal-top__title">{pcName}</h1>
        </div>
        {tools && <div className="journal-top__tools">{tools}</div>}
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
            {part === "timeline" && <Timeline view={view} actions={actions} pin={pin} />}
            {part === "decisions" && <Decisions view={view} pin={pin} />}
            {part === "open" && <OpenAndClosed view={view} pin={pin} />}
            {part === "before" && <KnownBefore lines={view.known_before} pin={pin} />}
            {part === "notes" && <Notes view={view} actions={actions} pin={pin} />}
          </div>
        </main>
      </div>
    </div>
  );
}

/** A pin toggle for one target, named for a screen reader by `label`. */
type Pin = (target: Target, label: string) => ReactNode;

function Empty({ children }: { children: ReactNode }) {
  return <p className="journal-empty">{children}</p>;
}

function Timeline({ view, actions, pin }: { view: JournalView; actions: JournalActions; pin: Pin }) {
  const [noting, setNoting] = useState<number | null>(null);
  const scenes = scenesNewestFirst(view);
  if (scenes.length === 0) return <Empty>The Journal starts when the story begins.</Empty>;
  return (
    <ol className="journal-timeline">
      {scenes.map(({ scene, number }) => {
        const target: Target = { kind: "scene", turn_id: scene.turn_id };
        const notes = notesAbout(view.notes, target);
        return (
          <li key={scene.turn_id} className="journal-scene">
            <span className="journal-scene__dot" aria-hidden="true" />
            <article aria-label={`Scene ${number}`}>
              <div className="journal-scene__head">
                <div>
                  <p className="play-label">
                    Scene {number} · {clockSpan(scene.clock_from, scene.clock_to)}
                  </p>
                  <h2 className="journal-scene__place">{placeTitle(scene.place)}</h2>
                </div>
                <div className="journal-scene__tools">
                  <button
                    type="button"
                    className="play-btn play-btn--text"
                    aria-expanded={noting === scene.turn_id}
                    onClick={() => setNoting(noting === scene.turn_id ? null : scene.turn_id)}
                  >
                    Note
                  </button>
                  {pin(target, `Scene ${number}`)}
                </div>
              </div>
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
                        {pin({ kind: "act", turn_id: act.turn_id }, act.intent)}
                      </li>
                    ))}
                  </ol>
                </section>
              )}
              {scene.learned.length > 0 && (
                <section className="journal-part">
                  <h3 className="play-label">What you learned</h3>
                  <Facts lines={scene.learned} pin={pin} />
                </section>
              )}
              {(notes.length > 0 || noting === scene.turn_id) && (
                <section className="journal-part">
                  <h3 className="play-label">Your notes</h3>
                  {notes.map((note) => (
                    <p key={note.id} className="journal-note-inline">
                      {note.body}
                    </p>
                  ))}
                  {noting === scene.turn_id && (
                    <NoteField
                      label={`Note about scene ${number}`}
                      placeholder="What this scene means to you"
                      onSave={(body) => actions.onAddNote(body, target)}
                      onClose={() => setNoting(null)}
                    />
                  )}
                </section>
              )}
            </article>
          </li>
        );
      })}
    </ol>
  );
}

function Decisions({ view, pin }: { view: JournalView; pin: Pin }) {
  const list = decisions(view);
  if (list.length === 0) return <Empty>No decision yet. A decision is an act that lasts, seen by you.</Empty>;
  return (
    <ol className="journal-decisions">
      {list.map(({ act, scene }) => (
        <li key={act.turn_id} className="journal-decision">
          <div className="journal-decision__main">
            <p className="journal-decision__intent">{act.intent}</p>
            <p className="journal-meta">
              {placeTitle(scene.place)} · {clockSpan(scene.clock_from, scene.clock_to)}
            </p>
          </div>
          {pin({ kind: "act", turn_id: act.turn_id }, act.intent)}
        </li>
      ))}
    </ol>
  );
}

function OpenAndClosed({ view, pin }: { view: JournalView; pin: Pin }) {
  return (
    <>
      <section className="journal-section">
        <h2 className="journal-section__title">Open</h2>
        <p className="journal-section__hint">Plans and pending matters you know of. Not done yet.</p>
        {view.open.length === 0 ? <Empty>Nothing open.</Empty> : <Facts lines={view.open} pin={pin} markOpen={false} />}
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

function KnownBefore({ lines, pin }: { lines: FactLine[]; pin: Pin }) {
  if (lines.length === 0) return <Empty>Nothing was known before the Campaign.</Empty>;
  return (
    <>
      <p className="journal-section__hint">What you knew when the Campaign began.</p>
      {bySubject(lines).map(({ subject, lines: facts }) => (
        <section key={subject.entity_id} className="journal-section journal-subject">
          <h2 className="journal-subject__name">
            <Who who={subject} />
          </h2>
          <Facts lines={facts} pin={pin} />
        </section>
      ))}
    </>
  );
}

/** The Notes part: a new free Note, then every Note, the latest first. */
function Notes({ view, actions, pin }: { view: JournalView; actions: JournalActions; pin: Pin }) {
  const [writing, setWriting] = useState(false);
  const notes = notesNewestFirst(view.notes);
  return (
    <>
      <div className="journal-notes__head">
        <p className="journal-section__hint">
          Your own notes. They stay with the Campaign. The GM never reads them.
        </p>
        {!writing && (
          <button type="button" className="play-btn play-btn--outline" onClick={() => setWriting(true)}>
            New note
          </button>
        )}
      </div>
      {writing && (
        <section className="journal-section">
          <NoteField
            label="New note"
            placeholder="A thought, a suspicion, a plan"
            onSave={(body) => actions.onAddNote(body, null)}
            onClose={() => setWriting(false)}
          />
        </section>
      )}
      {notes.length === 0 && !writing ? (
        <Empty>No notes yet. Write one here, from a Scene, from a card, or with Note in play.</Empty>
      ) : (
        <ol className="journal-notes">
          {notes.map((note) => (
            <NoteCard key={note.id} view={view} note={note} actions={actions} pin={pin} />
          ))}
        </ol>
      )}
    </>
  );
}

function NoteCard({ view, note, actions, pin }: { view: JournalView; note: Note; actions: JournalActions; pin: Pin }) {
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target: Target = { kind: "note", note_id: note.id };
  const pinned = isPinned(view.pins, target);

  const remove = async () => {
    const refused = await actions.onDeleteNote(note.id);
    if (refused !== null) setError(refused);
  };

  return (
    <li className={pinned ? "journal-note journal-note--pinned" : "journal-note"}>
      <div className="journal-note__head">
        <span className="journal-note__about">{aboutText(view, note.about)}</span>
        {pin(target, "this note")}
      </div>
      {editing ? (
        <NoteField
          label="Edit the note"
          initial={note.body}
          onSave={(body) => actions.onEditNote(note.id, body)}
          onClose={() => setEditing(false)}
        />
      ) : (
        <>
          <p className="journal-note__body">{note.body}</p>
          <div className="journal-note__tools">
            <span className="journal-meta">{note.updated_ms > note.created_ms ? "Edited" : "Written"}</span>
            <span className="journal-note__spacer" />
            <button type="button" className="play-btn play-btn--text" onClick={() => setEditing(true)}>
              Edit
            </button>
            {confirming ? (
              <>
                <button type="button" className="play-btn play-btn--text journal-note__delete" onClick={() => void remove()}>
                  Delete for good
                </button>
                <button type="button" className="play-btn play-btn--text" onClick={() => setConfirming(false)}>
                  Keep
                </button>
              </>
            ) : (
              <button type="button" className="play-btn play-btn--text" onClick={() => setConfirming(true)}>
                Delete
              </button>
            )}
          </div>
        </>
      )}
      {error !== null && (
        <p className="note-field__error" role="alert">
          {error}
        </p>
      )}
    </li>
  );
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

/**
 * Fact lines: an Open Item gets a ring and, unless `markOpen` is false, a
 * badge; a told fact names who told it; each line has a pin.
 */
function Facts({ lines, pin, markOpen = true }: { lines: FactLine[]; pin: Pin; markOpen?: boolean }) {
  return (
    <ul className="journal-facts">
      {lines.map((line) => (
        <li key={`${line.belief.subject_id}:${line.belief.key}`} className="journal-fact journal-fact--pinnable">
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
          {pin(factTarget(line), line.text)}
        </li>
      ))}
    </ul>
  );
}
