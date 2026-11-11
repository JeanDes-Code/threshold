import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { EntityCard, EntityKind, FactLine, KnownLine, NameRef, PinLine, Target, YouCard } from "../api";
import { factTarget, isPinned, notesNewestFirst } from "../marks/logic";
import NoteField from "../marks/NoteField";
import PinButton from "../marks/PinButton";
import { filterKnown, nameText, npcColor } from "./logic";

interface Props {
  here: NameRef[];
  you: YouCard;
  open: FactLine[];
  pinned: PinLine[];
  known: KnownLine[];
  /** The entity whose card is open, if any. */
  selected: number | null;
  /** The core's card of `selected`; null while it loads or when refused. */
  card: EntityCard | null;
  /** The core's refusal for `selected`. */
  cardRefusal: string | null;
  onSelect: (entityId: number) => void;
  onCloseCard: () => void;
  /** Pins or unpins one target; the core then sends a new view. */
  onPin: (target: Target, on: boolean) => void;
  /** Saves a Note about one target; resolves with the refusal text or null. */
  onNote: (body: string, about: Target) => Promise<string | null>;
}

/** What a Pin is, in one small word above its title. */
const PIN_KIND: Record<Target["kind"], string> = {
  scene: "Scene",
  act: "Act",
  belief: "Fact",
  entity: "Card",
  note: "Note",
};

const KIND_TEXT: Record<EntityKind, string> = {
  pc: "You",
  npc: "Person",
  place: "Place",
  faction: "Faction",
  item: "Thing",
};

/** Stacked cards. Each reads PC Knowledge and what the PC perceives now, nothing else. */
export default function SidePanel(props: Props) {
  const { here, you, open, pinned, known, selected, onSelect, onPin } = props;
  const pins = pinned.map((pin) => pin.target);
  return (
    <>
      {selected !== null && (
        <CardView
          key={selected}
          who={whoOf(selected, known, here)}
          card={props.card}
          refusal={props.cardRefusal}
          onClose={props.onCloseCard}
          pins={pins}
          onPin={onPin}
          onNote={props.onNote}
        />
      )}

      <Card title="Here">
        {here.length === 0 ? (
          <p className="play-empty">No one else.</p>
        ) : (
          <ul className="play-list">
            {here.map((who) => (
              <li key={who.entity_id} className="play-here">
                <Initial who={who} />
                <button
                  type="button"
                  className={who.name === null ? "play-name play-name--panel play-here__unknown" : "play-name play-name--panel"}
                  aria-current={who.entity_id === selected || undefined}
                  onClick={() => onSelect(who.entity_id)}
                >
                  {nameText(who)}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="You">
        <p className="play-you__name">{you.name}</p>
        <p className="play-you__concept">{you.concept}</p>
        {you.state.length > 0 && (
          <ul className="play-list play-you__state">
            {you.state.map((line) => (
              <li key={line.key} className="play-you__row">
                {line.text}
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="Open">
        {open.length === 0 ? (
          <p className="play-empty">Nothing open.</p>
        ) : (
          <Facts lines={open} pins={pins} onPin={onPin} />
        )}
      </Card>

      {pinned.length > 0 && (
        <Card title="Pinned">
          <ul className="play-list">
            {pinned.map((pin) => (
              <PinRow key={JSON.stringify(pin.target)} pin={pin} onSelect={onSelect} onPin={onPin} />
            ))}
          </ul>
        </Card>
      )}

      <People known={known} selected={selected} onSelect={onSelect} />
    </>
  );
}

/** The NameRef of an entity the panel lists, for the card head while the card loads. */
function whoOf(entityId: number, known: KnownLine[], here: NameRef[]): NameRef | null {
  return known.find((line) => line.who.entity_id === entityId)?.who ?? here.find((who) => who.entity_id === entityId) ?? null;
}

/** One line of the Pinned card. A card or a fact opens its card; the pin removes it. */
function PinRow({
  pin,
  onSelect,
  onPin,
}: {
  pin: PinLine;
  onSelect: (entityId: number) => void;
  onPin: (target: Target, on: boolean) => void;
}) {
  const { target } = pin;
  const opens = target.kind === "entity" ? target.entity_id : target.kind === "belief" ? target.subject_id : null;
  return (
    <li className={`play-pin play-pin--${target.kind}`}>
      <span className="play-pin__body">
        <span className="play-pin__kind">{PIN_KIND[target.kind]}</span>
        {opens !== null ? (
          <button type="button" className="play-pin__title play-pin__open" onClick={() => onSelect(opens)}>
            {pin.title}
          </button>
        ) : (
          <span className="play-pin__title">{pin.title}</span>
        )}
        {pin.text !== "" && <span className="play-pin__text">{pin.text}</span>}
      </span>
      <PinButton pinned label={pin.title} onToggle={(on) => onPin(target, on)} />
    </li>
  );
}

/** Fact lines: an Open Item gets a ring, a told fact names who told it, each has a pin. */
function Facts({
  lines,
  pins,
  onPin,
}: {
  lines: FactLine[];
  pins: Target[];
  onPin: (target: Target, on: boolean) => void;
}) {
  return (
    <ul className="play-list">
      {lines.map((line) => {
        const target = factTarget(line);
        return (
          <li key={`${line.belief.subject_id}:${line.belief.key}`} className="play-fact play-fact--pinnable">
            <span className={line.open ? "play-fact__mark play-fact__mark--open" : "play-fact__mark"} aria-hidden="true" />
            <span>
              {line.text}
              {line.open && <span className="play-sr"> (open)</span>}
              {line.told_by && <span className="play-faint"> · from {nameText(line.told_by)}</span>}
            </span>
            <PinButton pinned={isPinned(pins, target)} label={line.text} onToggle={(on) => onPin(target, on)} />
          </li>
        );
      })}
    </ul>
  );
}

interface CardViewProps {
  /** From the panel's lists; null for an entity they do not hold. */
  who: NameRef | null;
  card: EntityCard | null;
  refusal: string | null;
  onClose: () => void;
  pins: Target[];
  onPin: (target: Target, on: boolean) => void;
  onNote: (body: string, about: Target) => Promise<string | null>;
}

/** One entity as the PC knows it: what the core's card holds, nothing else. */
function CardView({ who: listed, card, refusal, onClose, pins, onPin, onNote }: CardViewProps) {
  const section = useRef<HTMLElement>(null);
  const [noting, setNoting] = useState(false);
  const who = card?.who ?? listed;
  const title = who ? nameText(who) : "Unknown";

  // A new card takes the focus, so the keyboard lands on it.
  useEffect(() => {
    section.current?.focus({ preventScroll: true });
    section.current?.scrollIntoView({ block: "nearest" });
  }, []);

  return (
    <section className="play-card play-entity" aria-label={`Card: ${title}`} tabIndex={-1} ref={section}>
      <div className="play-entity__head">
        {who && <Initial who={who} />}
        <div className="play-entity__title">
          <p className="play-label">
            {who ? KIND_TEXT[who.kind] : "Card"}
            {card?.here && <span className="play-entity__here"> · here</span>}
          </p>
          <h3 className={who?.name === null ? "play-entity__name play-entity__name--unknown" : "play-entity__name"}>
            {title}
          </h3>
        </div>
        {card !== null && (
          <PinButton
            pinned={card.pinned}
            label={title}
            onToggle={(on) => onPin({ kind: "entity", entity_id: card.who.entity_id }, on)}
          />
        )}
        <button type="button" className="play-entity__close" aria-label="Close the card" onClick={onClose}>
          ×
        </button>
      </div>
      {refusal !== null ? (
        <p className="play-empty">{refusal.charAt(0).toUpperCase() + refusal.slice(1)}.</p>
      ) : card === null ? (
        <p className="play-empty">Reading…</p>
      ) : card.who.name === null ? (
        <p className="play-empty">You see them, but you do not know their name.</p>
      ) : (
        <>
          {card.other_names.length > 0 && (
            <p className="play-entity__aka">
              Also known as <span>{card.other_names.join(", ")}</span>
            </p>
          )}
          {card.people_here.length > 0 && (
            <>
              <h4 className="play-label play-entity__part">Who you know is here</h4>
              <ul className="play-list play-entity__people">
                {card.people_here.map((person) => (
                  <li
                    key={person.entity_id}
                    className={person.name === null ? "play-entity__name--unknown" : undefined}
                  >
                    {nameText(person)}
                  </li>
                ))}
              </ul>
            </>
          )}
          <h4 className="play-label play-entity__part">What you know</h4>
          {card.facts.length > 0 ? (
            <Facts lines={card.facts} pins={pins} onPin={onPin} />
          ) : (
            card.named_in.length === 0 && <p className="play-empty">Nothing more yet.</p>
          )}
          {card.named_in.length > 0 && (
            <>
              <h4 className="play-label play-entity__part">Named in</h4>
              <Facts lines={card.named_in} pins={pins} onPin={onPin} />
            </>
          )}
          <h4 className="play-label play-entity__part play-entity__notes">Your notes</h4>
          {card.notes.length > 0 && (
            <ul className="play-list play-notes">
              {notesNewestFirst(card.notes).map((note) => (
                <li key={note.id} className="play-note">
                  {note.body}
                </li>
              ))}
            </ul>
          )}
          {noting ? (
            <NoteField
              label={`Note about ${title}`}
              placeholder={`What you make of ${title}`}
              onSave={(body) => onNote(body, { kind: "entity", entity_id: card.who.entity_id })}
              onClose={() => setNoting(false)}
            />
          ) : (
            <button type="button" className="play-btn play-btn--text play-entity__add" onClick={() => setNoting(true)}>
              + Add a note
            </button>
          )}
        </>
      )}
    </section>
  );
}

function Card({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="play-card" aria-label={title}>
      <h2 className="play-label">{title}</h2>
      {children}
    </section>
  );
}

function Initial({ who }: { who: NameRef }) {
  // A place or a Faction gets a plain marker: its first letter is often "T" of "The".
  if (who.kind !== "npc" && who.kind !== "pc") {
    return <span className="play-initial play-initial--thing" aria-hidden="true" />;
  }
  const style =
    who.kind === "npc" && who.name !== null
      ? ({ "--who": npcColor(who.entity_id) } as CSSProperties)
      : undefined;
  return (
    <span className="play-initial" style={style} aria-hidden="true">
      {who.name === null ? "?" : who.name.charAt(0).toUpperCase()}
    </span>
  );
}

interface PeopleProps {
  known: KnownLine[];
  selected: number | null;
  onSelect: (entityId: number) => void;
}

function People({ known, selected, onSelect }: PeopleProps) {
  const [query, setQuery] = useState("");
  const shown = filterKnown(known, query);

  return (
    <Card title="People and places">
      <input
        type="search"
        className="play-search"
        placeholder="Search"
        aria-label="Search people and places"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {shown.length === 0 ? (
        <p className="play-empty">No match.</p>
      ) : (
        <ul className="play-list play-people">
          {shown.map((line) => {
            const id = line.who.entity_id;
            const isOpen = id === selected;
            return (
              <li key={id} className={isOpen ? "play-person play-person--open" : "play-person"}>
                <button
                  type="button"
                  className="play-person__head"
                  aria-current={isOpen || undefined}
                  onClick={() => onSelect(id)}
                >
                  <Initial who={line.who} />
                  <span className="play-person__name">{nameText(line.who)}</span>
                  {line.here && <span className="play-person__here">here</span>}
                  <span className="play-person__kind">{KIND_TEXT[line.who.kind]}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}
