import { useState } from "react";
import type { PlayView } from "../api";
import ActionBar from "./ActionBar";
import { storyItems, turnPhase } from "./logic";
import SidePanel from "./SidePanel";
import Story from "./Story";
import TopBar from "./TopBar";

export interface PlayScreenProps {
  view: PlayView;
  /** True while a Turn runs. */
  busy: boolean;
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
  /** A Turn that committed with a problem, such as an interrupted narration. */
  notice: string | null;
  canRetry: boolean;
  onAct: (text: string) => Promise<boolean>;
  onAsk: (text: string) => Promise<boolean>;
  onCorrect: (clarified: string, why: string) => Promise<boolean>;
  onRetry: () => void;
  onBegin: () => void;
  onRenarrate: (turnId: number) => void;
  onDismissAnswer: () => void;
}

/** The Play screen. It reads one PlayView through props and holds only screen state. */
export default function PlayScreen(props: PlayScreenProps) {
  const { view, busy, stream, pendingIntent, answer } = props;
  const [selected, setSelected] = useState<number | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);

  const showName = (entityId: number) => {
    setSelected(entityId);
    setPanelOpen(true);
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
      />
      <div className="play__body">
        <Story
          items={storyItems(view, busy ? stream : null)}
          pendingIntent={pendingIntent}
          onName={showName}
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
            onSelect={setSelected}
          />
        </aside>
      </div>
      <ActionBar
        scene={view.scene}
        phase={turnPhase(busy, stream ?? "")}
        pcName={view.pc_name}
        needsBegin={view.needs_begin}
        answer={answer}
        failure={props.failure}
        notice={props.notice}
        canRetry={props.canRetry}
        canCorrect={view.can_correct}
        onAct={props.onAct}
        onAsk={props.onAsk}
        onCorrect={props.onCorrect}
        onRetry={props.onRetry}
        onBegin={props.onBegin}
        onDismissAnswer={props.onDismissAnswer}
      />
    </div>
  );
}
