import { useId, useState } from "react";
import type { SceneState } from "../api";
import NoteField from "../marks/NoteField";
import { PHASE_TEXT, isSendKey, type TurnPhase } from "./logic";

interface Props {
  /** The mature content module decides its buttons from `scene` and `phase`. */
  scene: SceneState;
  phase: TurnPhase;
  pcName: string;
  needsBegin: boolean;
  /** The OOC answer, shown apart from the story. */
  answer: string | null;
  /** Why the last Turn failed; nothing was committed. */
  failure: string | null;
  /** A Turn that committed with a problem, such as an interrupted narration. */
  notice: string | null;
  /** The failed Turn can be sent again under its op_id. */
  canRetry: boolean;
  /** The latest Turn is a standing Play Turn. */
  canCorrect: boolean;
  /** The Correction fields are open: the Campaign menu opens them. */
  fixing: boolean;
  onFixing: (open: boolean) => void;
  /** Resolves true when the Intent was taken: the field then clears. */
  onAct: (text: string) => Promise<boolean>;
  onAsk: (text: string) => Promise<boolean>;
  /** Resolves true when the Correction was committed: the fields then clear. */
  onCorrect: (clarified: string, why: string) => Promise<boolean>;
  onRetry: () => void;
  onBegin: () => void;
  onDismissAnswer: () => void;
  /** A Scene is shown: a quick Note can be about it. */
  hasScene: boolean;
  /** Saves a quick Note, about the current Scene or free; resolves with the refusal text or null. */
  onNote: (body: string, aboutScene: boolean) => Promise<string | null>;
}

/** The Intent field, "Act", quiet buttons, and an empty slot on the right for later modules. */
export default function ActionBar(props: Props) {
  const { scene, phase, pcName, needsBegin, answer, failure, notice, onAct, onAsk } = props;
  const { canRetry, canCorrect, fixing, onFixing, onCorrect, onRetry, onBegin, onDismissAnswer } = props;
  const [intent, setIntent] = useState("");
  const [ooc, setOoc] = useState("");
  const [asking, setAsking] = useState(false);
  const [clarified, setClarified] = useState("");
  const [why, setWhy] = useState("");
  const [noting, setNoting] = useState(false);
  const [aboutScene, setAboutScene] = useState(true);
  const oocId = useId();
  const fixId = useId();
  const noteId = useId();
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

  const correct = async () => {
    const text = clarified.trim();
    if (!text || !why.trim() || busy) return;
    if (await onCorrect(text, why.trim())) {
      onFixing(false);
      setClarified("");
      setWhy("");
    }
  };

  return (
    <footer className="play-bar" data-scene={scene.kind}>
      {failure !== null && (
        <section className="play-alert play-alert--error" role="alert">
          <p>The Turn failed. Nothing was committed.</p>
          <p className="play-alert__detail">{failure}</p>
          {canRetry && (
            <button type="button" className="play-btn play-btn--outline" disabled={busy} onClick={onRetry}>
              Retry
            </button>
          )}
        </section>
      )}
      {notice !== null && (
        <section className="play-alert" role="status">
          <p>{notice}</p>
        </section>
      )}

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
          <button
            type="button"
            className="play-btn play-btn--outline"
            aria-expanded={noting}
            aria-controls={noteId}
            onClick={() => setNoting((n) => !n)}
          >
            Note
          </button>
          <div className="action-bar__later" />
        </div>
      )}

      {fixing && !needsBegin && (
        <div className="play-bar__row play-bar__row--ooc" id={fixId} aria-label="Correct the last Turn">
          <span className="play-bar__label">Correct the last Turn</span>
          <input
            autoFocus
            className="play-ooc-field"
            aria-label="What you meant"
            placeholder="What you meant"
            value={clarified}
            onChange={(e) => setClarified(e.target.value)}
          />
          <input
            className="play-ooc-field"
            aria-label="Why the last Turn was wrong"
            placeholder="Why the last Turn was wrong"
            value={why}
            onChange={(e) => setWhy(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void correct();
              }
            }}
          />
          <button
            type="button"
            className="play-btn play-btn--outline"
            disabled={busy || !canCorrect || !clarified.trim() || !why.trim()}
            onClick={() => void correct()}
          >
            Apply Correction
          </button>
          <button type="button" className="play-btn play-btn--text" onClick={() => onFixing(false)}>
            Cancel
          </button>
        </div>
      )}

      {noting && !needsBegin && (
        <div className="play-bar__row play-bar__row--note" id={noteId}>
          <NoteField
            label="Your note"
            placeholder="A note for yourself. The GM never reads it."
            waiting={busy}
            onSave={(body) => props.onNote(body, props.hasScene && aboutScene)}
            onClose={() => setNoting(false)}
          >
            {props.hasScene && (
              <label className="note-field__about">
                <input type="checkbox" checked={aboutScene} onChange={(e) => setAboutScene(e.target.checked)} />
                About this scene
              </label>
            )}
          </NoteField>
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
