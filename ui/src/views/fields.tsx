// Inputs of the Seed Draft review. Each one takes the field path of the
// draft, so a gap's "Show me" button can find it by id.

import { useState } from "react";
import type { OtherName } from "../api";
import { fieldId, removeAt, replaceAt, splitList } from "../newCampaign";

interface TextProps {
  path: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** A text area of this many rows; an input line when absent. */
  rows?: number;
  /** No visible label: the label goes to screen readers only. */
  hidden?: boolean;
  placeholder?: string;
}

export function TextInput({ path, label, value, onChange, rows, hidden, placeholder }: TextProps) {
  const id = fieldId(path);
  const common = {
    id,
    value,
    placeholder,
    "aria-label": hidden ? label : undefined,
  };
  return (
    <>
      {!hidden && <label htmlFor={id}>{label}</label>}
      {rows ? (
        <textarea {...common} rows={rows} onChange={(e) => onChange(e.target.value)} />
      ) : (
        <input {...common} onChange={(e) => onChange(e.target.value)} />
      )}
    </>
  );
}

interface ListProps {
  path: string;
  label: string;
  value: string[];
  onChange: (value: string[]) => void;
  /** "," for one line, "\n" for a text area with one item per line. */
  separator: "," | "\n";
  hidden?: boolean;
  placeholder?: string;
}

/**
 * A list typed as text. While the Player types, the input keeps the raw
 * text, so a trailing comma or an empty line does not vanish.
 */
export function ListInput({
  path,
  label,
  value,
  onChange,
  separator,
  hidden,
  placeholder,
}: ListProps) {
  const [text, setText] = useState<string | null>(null);
  const id = fieldId(path);
  const common = {
    id,
    value: text ?? value.join(separator === "," ? ", " : "\n"),
    placeholder,
    "aria-label": hidden ? label : undefined,
    onBlur: () => setText(null),
    onChange: (e: { target: { value: string } }) => {
      setText(e.target.value);
      onChange(splitList(e.target.value, separator));
    },
  };
  return (
    <>
      {!hidden && <label htmlFor={id}>{label}</label>}
      {separator === "\n" ? <textarea {...common} rows={3} /> : <input {...common} />}
    </>
  );
}

interface AgeProps {
  path: string;
  label: string;
  value: number | null;
  /** Empty means "no age" (null); else an empty input keeps the last age. */
  nullable: boolean;
  onChange: (value: number | null) => void;
}

/** An age in whole years. The aria label names it; a visible label is the caller's. */
export function AgeInput({ path, label, value, nullable, onChange }: AgeProps) {
  const [text, setText] = useState<string | null>(null);
  return (
    <input
      id={fieldId(path)}
      className="number"
      inputMode="numeric"
      aria-label={label}
      value={text ?? (value === null ? "" : String(value))}
      onBlur={() => setText(null)}
      onChange={(e) => {
        const raw = e.target.value;
        setText(raw);
        const n = Number(raw.trim());
        if (raw.trim() === "") {
          if (nullable) onChange(null);
        } else if (Number.isInteger(n) && n >= 0 && n <= 65535) {
          onChange(n);
        }
      }}
    />
  );
}

interface NameProps {
  path: string;
  label: string;
  /** What the field shows: the typed name, else the saved one. */
  value: string;
  /** The name the store holds. */
  saved: string;
  onType: (text: string) => void;
  /** Enter or leaving the field: the caller renames or saves. */
  onCommit: () => void;
  /** Esc: back to the saved name. */
  onCancel: () => void;
  /** The core refused the last rename: why, in one sentence. */
  error?: string | null;
  hidden?: boolean;
}

/**
 * A main name. Typing changes nothing in the draft: Enter or leaving the
 * field asks the core to rename the entry everywhere it is named.
 */
export function NameInput({ path, label, value, saved, onType, onCommit, onCancel, error, hidden }: NameProps) {
  const id = fieldId(path);
  const changed = value.trim() !== saved;
  const hint = `${id}-hint`;
  return (
    <div className="review-name">
      {!hidden && <label htmlFor={id}>{label}</label>}
      <input
        id={id}
        value={value}
        aria-label={hidden ? label : undefined}
        aria-describedby={changed || error ? hint : undefined}
        aria-invalid={error ? true : undefined}
        onChange={(e) => onType(e.target.value)}
        onBlur={onCommit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            onCommit();
          } else if (e.key === "Escape") {
            onCancel();
          }
        }}
      />
      {error ? (
        <p id={hint} className="review-gap" role="alert">
          {error}
        </p>
      ) : (
        changed && (
          <p id={hint} className="review-hint">
            {saved.trim() === ""
              ? "Press Enter to save this name."
              : `Press Enter to rename “${saved}” everywhere in the draft. Esc keeps the old name.`}
          </p>
        )
      )}
    </div>
  );
}

interface OtherNamesProps {
  path: string;
  /** Whose names these are, for the labels: "Agathe Morrow", "your character". */
  owner: string;
  value: OtherName[];
  /** `now`: an add or a delete, saved at once. */
  onChange: (value: OtherName[], now: boolean) => void;
}

/** The other names of an entry, each with who knows it. Every box has a label and the full width. */
export function OtherNames({ path, owner, value, onChange }: OtherNamesProps) {
  const set = (j: number, patch: Partial<OtherName>) => onChange(replaceAt(value, j, patch), false);
  return (
    <div className="review-others">
      <p className="review-others__title">Other names</p>
      {value.length === 0 && <p className="review-hint">No other name for {owner}.</p>}
      {value.map((o, j) => (
        <div key={j} className="review-others__row">
          <div className="review-others__field">
            <TextInput
              path={`${path}[${j}].name`}
              label={`Other name ${j + 1}`}
              value={o.name}
              onChange={(name) => set(j, { name })}
            />
          </div>
          <div className="review-others__field review-others__field--wide">
            <ListInput
              path={`${path}[${j}].known_to`}
              label={`Who knows that “${o.name || "this name"}” is ${owner} (comma between names)`}
              separator=","
              value={o.known_to}
              onChange={(known_to) => set(j, { known_to })}
            />
          </div>
          <button
            type="button"
            className="play-btn play-btn--text review-others__delete"
            aria-label={`Delete the other name ${o.name || j + 1}`}
            onClick={() => onChange(removeAt(value, j), true)}
          >
            Delete
          </button>
        </div>
      ))}
      <button
        type="button"
        className="play-btn play-btn--text review-add-name"
        onClick={() => onChange([...value, { name: "", known_to: [] }], true)}
      >
        + Add another name
      </button>
    </div>
  );
}
