import { useId, useState } from "react";
import type { SceneState } from "../api";
import { isSendKey, type TurnPhase } from "./logic";

interface Props {
  /** The mature content module decides its buttons from `scene` and `phase`. */
  scene: SceneState;
  phase: TurnPhase;
  pcName: string;
  needsBegin: boolean;
  /** The OOC answer, shown apart from the story. */
  answer: string | null;
  /** Resolves true when the Intent was taken: the field then clears. */
  onAct: (text: string) => Promise<boolean>;
  onAsk: (text: string) => Promise<boolean>;
  onBegin: () => void;
  onDismissAnswer: () => void;
}

const PHASE_TEXT: Record<TurnPhase, string> = {
  idle: "",
  deciding: "The GM decides",
  narrating: "The GM narrates",
};

/** The Intent field, "Act", quiet buttons, and an empty slot on the right for later modules. */
export default function ActionBar(props: Props) {
  const { scene, phase, pcName, needsBegin, answer, onAct, onAsk, onBegin, onDismissAnswer } = props;
  const [intent, setIntent] = useState("");
  const [ooc, setOoc] = useState("");
  const [asking, setAsking] = useState(false);
  const oocId = useId();
  const busy = phase !== "idle";

  const act = async () => {
    const text = intent.trim();
    if (!text || busy) return;
    if (await onAct(text)) setIntent("");
  };

  const ask = async () => {
    const text = ooc.trim();
    if (!text || busy) return;
    if (await onAsk(text)) setOoc("");
  };

  return (
    <footer className="play-bar" data-scene={scene.kind}>
      {answer !== null && (
        <section className="play-ooc-answer" aria-label="Answer from the GM, out of character">
          <div className="play-ooc-answer__head">
            <span className="play-label">Out of character</span>
            <button type="button" className="play-btn play-btn--text" onClick={onDismissAnswer}>
              Close
            </button>
          </div>
          <p>{answer}</p>
        </section>
      )}

      <p className={busy ? "play-phase play-phase--on" : "play-phase"} role="status">
        {busy && <span className="play-phase__dot" aria-hidden="true" />}
        {PHASE_TEXT[phase]}
      </p>

      {needsBegin ? (
        <div className="play-bar__row">
          <p className="play-bar__hint">The GM sets the opening scene.</p>
          <button type="button" className="play-btn play-btn--primary" disabled={busy} onClick={onBegin}>
            Begin
          </button>
          <div className="action-bar__later" />
        </div>
      ) : (
        <div className="play-bar__row">
          <textarea
            className="play-intent-field"
            rows={2}
            aria-label={`What does ${pcName} do?`}
            placeholder={`What does ${pcName} do?`}
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
            onKeyDown={(e) => {
              if (isSendKey(e)) {
                e.preventDefault();
                void act();
              }
            }}
          />
          <button
            type="button"
            className="play-btn play-btn--primary"
            disabled={busy || !intent.trim()}
            onClick={() => void act()}
          >
            Act
          </button>
          <button
            type="button"
            className="play-btn play-btn--outline"
            aria-expanded={asking}
            aria-controls={oocId}
            onClick={() => setAsking((a) => !a)}
          >
            Ask the GM
          </button>
          <div className="action-bar__later" />
        </div>
      )}

      {asking && (
        <div className="play-bar__row play-bar__row--ooc" id={oocId}>
          <input
            className="play-ooc-field"
            aria-label="Question for the GM, out of character"
            placeholder="Ask the GM a question outside the story"
            value={ooc}
            onChange={(e) => setOoc(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void ask();
              }
            }}
          />
          <button
            type="button"
            className="play-btn play-btn--outline"
            disabled={busy || !ooc.trim()}
            onClick={() => void ask()}
          >
            Ask
          </button>
        </div>
      )}
    </footer>
  );
}
