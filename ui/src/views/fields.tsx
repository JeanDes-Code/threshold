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

interface OtherNamesProps {
  path: string;
  value: OtherName[];
  /** `now`: an add or a delete, saved at once. */
  onChange: (value: OtherName[], now: boolean) => void;
}

/** A character's other names, and who knows that each one names them. */
export function OtherNames({ path, value, onChange }: OtherNamesProps) {
  const set = (j: number, patch: Partial<OtherName>) => onChange(replaceAt(value, j, patch), false);
  return (
    <div className="others">
      {value.map((o, j) => (
        <div key={j} className="row">
          <TextInput
            path={`${path}[${j}].name`}
            label="Other name"
            hidden
            placeholder="Other name"
            value={o.name}
            onChange={(name) => set(j, { name })}
          />
          <ListInput
            path={`${path}[${j}].known_to`}
            label={`Who knows that ${o.name || "this name"} is the same person`}
            hidden
            placeholder="Who knows it (comma between names)"
            separator=","
            value={o.known_to}
            onChange={(known_to) => set(j, { known_to })}
          />
          <button
            aria-label={`Delete ${o.name || "this name"}`}
            onClick={() => onChange(removeAt(value, j), true)}
          >
            Delete
          </button>
        </div>
      ))}
      <button
        className="muted"
        onClick={() => onChange([...value, { name: "", known_to: [] }], true)}
      >
        Add a name
      </button>
    </div>
  );
}
