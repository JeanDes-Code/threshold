import { useLayoutEffect, useRef, type CSSProperties } from "react";
import { rollText, type Block, type FoundCard, type Part, type StoryTurn } from "../api";
import {
  emphasis,
  lookCloserDraft,
  nameText,
  npcColor,
  shouldFollow,
  streamShown,
  tabbableTurn,
  type StoryItem,
} from "./logic";

interface Props {
  items: StoryItem[];
  /** The Intent of the Turn that runs now, shown before its stream. */
  pendingIntent: string | null;
  onName: (entityId: number) => void;
  onRenarrate: (turnId: number) => void;
  /** A row of a found card: puts this Intent draft in the field, unsent. */
  onDraft: (draft: string) => void;
  busy: boolean;
}

/** The story column: each Turn's Intent, then its Narration. Follows the stream unless scrolled up. */
export default function Story({ items, pendingIntent, onName, onRenarrate, onDraft, busy }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const follow = useRef(true);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [items]);

  const onScroll = () => {
    const el = scroller.current;
    if (el) follow.current = shouldFollow(el.scrollTop, el.clientHeight, el.scrollHeight);
  };

  const tabbable = tabbableTurn(items);

  return (
    <div className="play-story" ref={scroller} onScroll={onScroll}>
      <div className="play-story__measure">
        {items.map((item) =>
          item.kind === "turn" ? (
            <Turn
              key={item.turn.turn_id}
              turn={item.turn}
              onName={onName}
              onRenarrate={onRenarrate}
              onDraft={onDraft}
              busy={busy}
              tabbable={item.turn.turn_id === tabbable}
            />
          ) : (
            <article key="stream" className="play-turn play-turn--live" aria-live="polite">
              {pendingIntent && <Intent text={pendingIntent} />}
              <Stream text={item.text} />
            </article>
          ),
        )}
      </div>
    </div>
  );
}

/** The raw Narration while it streams; a quiet "…" before its first chunk. */
function Stream({ text }: { text: string }) {
  const shown = streamShown(text);
  return (
    <div className={shown.waiting ? "play-stream play-stream--waiting" : "play-stream"}>
      {shown.waiting ? <span aria-label="The GM is deciding">{shown.text}</span> : shown.text}
      <span className="play-stream__caret" aria-hidden="true" />
    </div>
  );
}

function Intent({ text, correction = false }: { text: string; correction?: boolean }) {
  return (
    <p className="play-intent">
      <span className="play-intent__mark" aria-hidden="true">
        {correction ? "↺" : "›"}
      </span>
      {correction && <span className="play-intent__label">Correction</span>}
      <span className="play-intent__text">{text}</span>
    </p>
  );
}

interface TurnProps {
  turn: StoryTurn;
  onName: (entityId: number) => void;
  onRenarrate: (turnId: number) => void;
  onDraft: (draft: string) => void;
  busy: boolean;
  /** Its name links and found rows are in the tab order: only the last Turn's are. */
  tabbable: boolean;
}

function Turn({ turn, onName, onRenarrate, onDraft, busy, tabbable }: TurnProps) {
  const classes = ["play-turn"];
  if (turn.reverted) classes.push("play-turn--reverted");
  return (
    <article className={classes.join(" ")}>
      {/* An Opening or Closing Turn has no Intent: only its Narration shows. */}
      {(turn.kind === "play" || turn.kind === "correction") && (
        <Intent text={turn.intent} correction={turn.kind === "correction"} />
      )}
      {turn.reverted && <p className="play-turn__tag">Reverted by the Correction below</p>}
      {turn.body.kind === "narration" ? (
        turn.body.blocks.map((block, i) => (
          <BlockView key={i} block={block} onName={onName} tabbable={tabbable} />
        ))
      ) : turn.body.kind === "faded" ? (
        <p className="play-faded">{turn.body.line}</p>
      ) : (
        <div className="play-missing">
          <span>The Narration of this Turn is missing. The Turn itself is committed.</span>
          <button
            type="button"
            className="play-btn play-btn--quiet"
            disabled={busy}
            onClick={() => onRenarrate(turn.turn_id)}
          >
            Narrate again
          </button>
        </div>
      )}
      {turn.found && <Found card={turn.found} onDraft={onDraft} tabbable={tabbable} />}
      {turn.roll && (
        <p className={`play-roll ${turn.roll.success ? "play-roll--ok" : "play-roll--bad"}`}>
          {rollText(turn.roll)}
        </p>
      )}
    </article>
  );
}

/**
 * What the PC found in this Turn: a quiet card titled with the source, one row
 * per item. A row puts a short Intent draft in the field; it sends nothing.
 */
function Found({
  card,
  onDraft,
  tabbable,
}: {
  card: FoundCard;
  onDraft: (draft: string) => void;
  tabbable: boolean;
}) {
  return (
    <section className="play-found" aria-label={`Found in ${card.source}`}>
      <p className="play-label play-found__source">{card.source}</p>
      <ul className="play-found__list">
        {card.items.map((row, i) => (
          <li key={i}>
            <button
              type="button"
              className="play-found__row"
              tabIndex={tabbable ? undefined : -1}
              title="Put a draft in the Intent field"
              onClick={() => onDraft(lookCloserDraft(row.title))}
            >
              <span className="play-found__title">{row.title}</span>
              {row.line && <span className="play-found__line">{row.line}</span>}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

interface Links {
  onName: (entityId: number) => void;
  tabbable: boolean;
}

function BlockView({ block, onName, tabbable }: { block: Block } & Links) {
  if (block.kind === "prose") {
    return (
      <p className="play-prose">
        <Parts parts={block.parts} onName={onName} tabbable={tabbable} />
      </p>
    );
  }
  const who = nameText(block.speaker);
  const style = { "--who": npcColor(block.speaker.entity_id) } as CSSProperties;
  return (
    <div className="play-speech" style={style}>
      <span className="play-initial play-speech__initial" aria-hidden="true">
        {who.charAt(0).toUpperCase()}
      </span>
      <div className="play-speech__body">
        <button
          type="button"
          className="play-speech__name"
          tabIndex={tabbable ? undefined : -1}
          onClick={() => onName(block.speaker.entity_id)}
        >
          {who}
        </button>
        {block.stage && (
          <p className="play-speech__stage">
            (<Parts parts={block.stage} onName={onName} tabbable={tabbable} />)
          </p>
        )}
        <p className="play-speech__words">
          <Parts parts={block.parts} onName={onName} tabbable={tabbable} />
        </p>
      </div>
    </div>
  );
}

function Parts({ parts, onName, tabbable }: { parts: Part[] } & Links) {
  return parts.map((part, i) =>
    part.kind === "text" ? (
      <span key={i}>
        {emphasis(part.text).map((piece, j) => (piece.em ? <em key={j}>{piece.text}</em> : piece.text))}
      </span>
    ) : (
      <button
        key={i}
        type="button"
        className="play-name"
        tabIndex={tabbable ? undefined : -1}
        onClick={() => onName(part.entity_id)}
      >
        {part.text}
      </button>
    ),
  );
}
