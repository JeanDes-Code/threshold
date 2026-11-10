import { useLayoutEffect, useRef, type CSSProperties } from "react";
import { rollText, type Block, type Part, type StoryTurn } from "../api";
import { nameText, npcColor, shouldFollow, streamShown, type StoryItem } from "./logic";

interface Props {
  items: StoryItem[];
  /** The Intent of the Turn that runs now, shown before its stream. */
  pendingIntent: string | null;
  onName: (entityId: number) => void;
  onRenarrate: (turnId: number) => void;
  busy: boolean;
}

/** The story column: each Turn's Intent, then its Narration. Follows the stream unless scrolled up. */
export default function Story({ items, pendingIntent, onName, onRenarrate, busy }: Props) {
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
              busy={busy}
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
  busy: boolean;
}

function Turn({ turn, onName, onRenarrate, busy }: TurnProps) {
  const classes = ["play-turn"];
  if (turn.reverted) classes.push("play-turn--reverted");
  return (
    <article className={classes.join(" ")}>
      {/* An Opening Turn has no Intent: only its Narration shows. */}
      {turn.kind !== "opening" && <Intent text={turn.intent} correction={turn.kind === "correction"} />}
      {turn.reverted && <p className="play-turn__tag">Reverted by the Correction below</p>}
      {turn.body.kind === "narration" ? (
        turn.body.blocks.map((block, i) => <BlockView key={i} block={block} onName={onName} />)
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
      {turn.roll && (
        <p className={`play-roll ${turn.roll.success ? "play-roll--ok" : "play-roll--bad"}`}>
          {rollText(turn.roll)}
        </p>
      )}
    </article>
  );
}

function BlockView({ block, onName }: { block: Block; onName: (entityId: number) => void }) {
  if (block.kind === "prose") {
    return (
      <p className="play-prose">
        <Parts parts={block.parts} onName={onName} />
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
        <button type="button" className="play-speech__name" onClick={() => onName(block.speaker.entity_id)}>
          {who}
        </button>
        <p className="play-speech__words">
          <Parts parts={block.parts} onName={onName} />
        </p>
      </div>
    </div>
  );
}

function Parts({ parts, onName }: { parts: Part[]; onName: (entityId: number) => void }) {
  return parts.map((part, i) =>
    part.kind === "text" ? (
      <span key={i}>{part.text}</span>
    ) : (
      <button key={i} type="button" className="play-name" onClick={() => onName(part.entity_id)}>
        {part.text}
      </button>
    ),
  );
}
