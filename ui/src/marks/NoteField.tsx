import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { NOTE_MAX_CHARS, isSaveKey, noteLength, noteProblem } from "./logic";

interface Props {
  /** The field's name for a screen reader. */
  label: string;
  initial?: string;
  placeholder?: string;
  saveText?: string;
  /** True while the core cannot take a Note (a Turn runs). */
  waiting?: boolean;
  /** Saves the body; resolves with the core's refusal text, or null when saved. */
  onSave: (body: string) => Promise<string | null>;
  onClose: () => void;
  /** Extra controls next to the buttons, such as "about this Scene". */
  children?: ReactNode;
}

/** A Note field. Escape closes it; Ctrl+Enter or the button saves it. */
export default function NoteField(props: Props) {
  const { label, initial = "", placeholder, saveText = "Save note", waiting = false, onSave, onClose } = props;
  const [body, setBody] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const hintId = useId();

  useEffect(() => {
    const el = field.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, []);

  const problem = noteProblem(body);
  const length = noteLength(body);
  const save = async () => {
    if (problem !== null) {
      setError(body.trim() === "" ? null : problem);
      return;
    }
    if (saving || waiting) return;
    setSaving(true);
    const refused = await onSave(body);
    setSaving(false);
    if (refused === null) {
      setBody("");
      onClose();
    } else {
      setError(refused);
    }
  };

  return (
    <div className="note-field">
      <textarea
        ref={field}
        className="note-field__text"
        aria-label={label}
        aria-describedby={hintId}
        placeholder={placeholder}
        rows={3}
        value={body}
        onChange={(e) => {
          setBody(e.target.value);
          setError(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          } else if (isSaveKey(e)) {
            e.preventDefault();
            void save();
          }
        }}
      />
      <div className="note-field__row">
        {props.children}
        <span id={hintId} className={length > NOTE_MAX_CHARS ? "note-field__hint note-field__hint--over" : "note-field__hint"}>
          {waiting
            ? "You can save when the GM is done."
            : length > NOTE_MAX_CHARS * 0.9
              ? `${length} / ${NOTE_MAX_CHARS}`
              : "Ctrl+Enter saves · Esc closes"}
        </span>
        <button type="button" className="play-btn play-btn--text" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="play-btn play-btn--outline note-field__save"
          disabled={saving || waiting || problem !== null}
          onClick={() => void save()}
        >
          {saving ? "Saving…" : saveText}
        </button>
      </div>
      {error !== null && (
        <p className="note-field__error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
