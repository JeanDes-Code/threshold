import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { EntityKind, FactLine, KnownLine, NameRef, PinLine, YouCard } from "../api";
import { matchesSearch, nameText, npcColor } from "./logic";

interface Props {
  here: NameRef[];
  you: YouCard;
  open: FactLine[];
  pinned: PinLine[];
  known: KnownLine[];
  /** The entity whose entry is open in "People and places". */
  selected: number | null;
  onSelect: (entityId: number | null) => void;
}

const KIND_TEXT: Record<EntityKind, string> = {
  pc: "You",
  npc: "Person",
  place: "Place",
  faction: "Faction",
  item: "Thing",
};

/** Stacked cards. Each reads PC Knowledge and what the PC perceives now, nothing else. */
export default function SidePanel({ here, you, open, pinned, known, selected, onSelect }: Props) {
  return (
    <>
      <Card title="Here">
        {here.length === 0 ? (
          <p className="play-empty">No one else.</p>
        ) : (
          <ul className="play-list">
            {here.map((who) => (
              <li key={who.entity_id} className="play-here">
                <Initial who={who} />
                {who.name === null ? (
                  <span className="play-here__unknown">unknown</span>
                ) : (
                  <button type="button" className="play-name play-name--panel" onClick={() => onSelect(who.entity_id)}>
                    {who.name}
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="You">
        <p className="play-you__name">{you.name}</p>
        <p className="play-you__concept">{you.concept}</p>
        {you.state.length > 0 && (
          <dl className="play-you__state">
            {you.state.map((line) => (
              <div key={line.key} className="play-you__row">
                <dt>{line.key}</dt>
                <dd>{line.value}</dd>
              </div>
            ))}
          </dl>
        )}
      </Card>

      <Card title="Open">
        {open.length === 0 ? (
          <p className="play-empty">Nothing open.</p>
        ) : (
          <ul className="play-list">
            {open.map((line) => (
              <li key={`${line.belief.subject_id}:${line.belief.key}`} className="play-open">
                <span className="play-open__mark" aria-hidden="true" />
                <span>
                  {line.text}
                  {line.told_by && <span className="play-faint"> · from {nameText(line.told_by)}</span>}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>

      {pinned.length > 0 && (
        <Card title="Pinned">
          <ul className="play-list">
            {pinned.map((pin) => (
              <li key={JSON.stringify(pin.target)} className="play-pin">
                <span className="play-pin__title">{pin.title}</span>
                <span className="play-pin__text">{pin.text}</span>
              </li>
            ))}
          </ul>
        </Card>
      )}

      <People known={known} selected={selected} onSelect={onSelect} />
    </>
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
  onSelect: (entityId: number | null) => void;
}

function People({ known, selected, onSelect }: PeopleProps) {
  const [query, setQuery] = useState("");
  const list = useRef<HTMLUListElement>(null);
  const shown = known.filter((line) => line.who.entity_id === selected || matchesSearch(line, query));

  useEffect(() => {
    if (selected === null) return;
    list.current?.querySelector(`[data-entity="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

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
        <ul className="play-list play-people" ref={list}>
          {shown.map((line) => {
            const id = line.who.entity_id;
            const isOpen = id === selected;
            return (
              <li key={id} data-entity={id} className={isOpen ? "play-person play-person--open" : "play-person"}>
                <button
                  type="button"
                  className="play-person__head"
                  aria-expanded={isOpen}
                  onClick={() => onSelect(isOpen ? null : id)}
                >
                  <Initial who={line.who} />
                  <span className="play-person__name">{nameText(line.who)}</span>
                  {line.here && <span className="play-person__here">here</span>}
                  <span className="play-person__kind">{KIND_TEXT[line.who.kind]}</span>
                </button>
                {isOpen && (
                  <div className="play-person__card">
                    {line.other_names.length > 0 ? (
                      <p>Also known as {line.other_names.join(", ")}.</p>
                    ) : (
                      <p className="play-faint">No other name known.</p>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}
