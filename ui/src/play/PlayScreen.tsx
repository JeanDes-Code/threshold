import { useEffect, useState, type ReactNode } from "react";
import { errorText, type EntityCard, type PlayView, type Target } from "../api";
import { currentScene } from "../marks/logic";
import ActionBar from "./ActionBar";
import { storyItems, turnPhase } from "./logic";
import SidePanel from "./SidePanel";
import Story from "./Story";
import TopBar from "./TopBar";

export interface PlayScreenProps {
  view: PlayView;
  /** True while a Turn runs. */
  busy: boolean;
  /** When the running call started (the session store keeps it); null while idle. */
  startedMs: number | null;
  /**
   * The raw Narration text of the running Turn, "" before its first chunk;
   * null when no Turn runs (an OOC question keeps the story as it is).
   */
  stream: string | null;
  /** The Intent of the running Turn. */
  pendingIntent: string | null;
  answer: string | null;
  /** Why the last Turn failed; nothing was committed. */
  failure: string | null;
  /** The sentence of the last Refusal: not a failure, no retry. */
  refusal: string | null;
  /** A Turn that committed with a problem, such as an interrupted narration. */
  notice: string | null;
  canRetry: boolean;
  /** `get_entity_card`: what the PC knows about one entity. */
  loadCard: (entityId: number) => Promise<EntityCard>;
  onAct: (text: string) => Promise<boolean>;
  onAsk: (text: string) => Promise<boolean>;
  onCorrect: (clarified: string, why: string) => Promise<boolean>;
  /** Sends the failed Turn again; resolves true when it committed. */
  onRetry: () => Promise<boolean>;
  /** Closes the failure, and forgets the Turn that could be sent again. */
  onDismissFailure: () => void;
  onBegin: () => void;
  /** "End scene": the Closing Turn of the open Intimate Scene. */
  onEndScene: () => void;
  /** "Fade to black". */
  onFade: () => void;
  /** The Player faded the running Narration: its text is off the screen. */
  faded?: boolean;
  onRenarrate: (turnId: number) => void;
  onDismissAnswer: () => void;
  /** Pins or unpins a target; the core then sends a new view. */
  onPin: (target: Target, on: boolean) => void;
  /** Saves a Note; resolves with the core's refusal text, or null when saved. */
  onNote: (body: string, about: Target | null) => Promise<string | null>;
  /** The Correction fields are open (the Campaign menu opens them). */
  fixing: boolean;
  onFixing: (open: boolean) => void;
  /** The screen switch and the Campaign menu, on the right of the top bar. */
  tools?: ReactNode;
}

/** The last answer of the core for one entity: its card, or the refusal text. */
interface CardAnswer {
  entityId: number;
  card: EntityCard | null;
  refusal: string | null;
}

/** The Play screen. It reads one PlayView through props and holds only screen state. */
export default function PlayScreen(props: PlayScreenProps) {
  const { view, busy, stream, pendingIntent, answer, loadCard } = props;
  const [selected, setSelected] = useState<number | null>(null);
  const [card, setCard] = useState<CardAnswer | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);

  // The card reads the core again after each Turn: what the PC knows changes.
  // Until the answer comes, the panel keeps the last card of that entity.
  useEffect(() => {
    if (selected === null) return;
    let current = true;
    loadCard(selected).then(
      (found) => {
        if (current) setCard({ entityId: selected, card: found, refusal: null });
      },
      (e: unknown) => {
        if (current) setCard({ entityId: selected, card: null, refusal: errorText(e) });
      },
    );
    return () => {
      current = false;
    };
  }, [selected, view, loadCard]);
  const shown = card !== null && card.entityId === selected ? card : null;
  const scene = currentScene(view);

  const showCard = (entityId: number) => {
    setSelected(entityId);
    setPanelOpen(true);
  };

  const closeCard = () => {
    setSelected(null);
    setCard(null);
  };

  return (
    <div
      className={panelOpen ? "play play--panel-open" : "play"}
      onKeyDown={(e) => {
        if (e.key === "Escape" && panelOpen) setPanelOpen(false);
      }}
    >
      <TopBar
        place={view.place}
        clockMin={view.clock_min}
        panelOpen={panelOpen}
        onTogglePanel={() => setPanelOpen((o) => !o)}
        tools={props.tools}
      />
      <div className="play__body">
        <Story
          items={storyItems(view, busy && !props.faded ? stream : null)}
          pendingIntent={pendingIntent}
          onName={showCard}
          onRenarrate={props.onRenarrate}
          busy={busy}
        />
        <aside className="play-panel" id="play-panel" aria-label="What you know">
          <SidePanel
            here={view.here}
            you={view.you}
            open={view.open}
            pinned={view.pinned}
            known={view.known}
            selected={selected}
            card={shown?.card ?? null}
            cardRefusal={shown?.refusal ?? null}
            onSelect={showCard}
            onCloseCard={closeCard}
            onPin={props.onPin}
            onNote={props.onNote}
          />
        </aside>
      </div>
      <ActionBar
        scene={view.scene}
        phase={props.faded && busy ? "fading" : turnPhase(busy, stream)}
        startedMs={props.startedMs}
        pcName={view.pc_name}
        needsBegin={view.needs_begin}
        answer={answer}
        failure={props.failure}
        refusal={props.refusal}
        notice={props.notice}
        canRetry={props.canRetry}
        canCorrect={view.can_correct}
        fixing={props.fixing}
        onFixing={props.onFixing}
        onAct={props.onAct}
        onAsk={props.onAsk}
        onCorrect={props.onCorrect}
        onRetry={props.onRetry}
        onDismissFailure={props.onDismissFailure}
        onBegin={props.onBegin}
        onEndScene={props.onEndScene}
        onFade={props.onFade}
        onDismissAnswer={props.onDismissAnswer}
        hasScene={scene !== null}
        onNote={(body, aboutScene) => props.onNote(body, aboutScene ? scene : null)}
      />
    </div>
  );
}
