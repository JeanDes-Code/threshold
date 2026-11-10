import type { ReactNode } from "react";

interface Props {
  text: string;
  onText: (text: string) => void;
  onFile: (file: File) => void;
  onDraft: () => void;
  /** A draft or a build runs: nothing here may change. */
  busy: boolean;
  /** The progress line of a running draft. */
  running: ReactNode;
  /** The shell's sentence for a refused text, or another error. */
  error: string | null;
}

/** The first step: the Source Text, pasted or read from a file. The core checks its length. */
export default function SourceStep({ text, onText, onFile, onDraft, busy, running, error }: Props) {
  const count = text.trim().length === 0 ? 0 : [...text.trim()].length;
  return (
    <div className="source-step">
      <h2 className="review-intro__title">Start from your document</h2>
      <p className="review-lead">
        Paste your Source Text, or choose a .md or .txt file. The app reads it and drafts the Adventure Seed. You
        check the draft before anything else happens.
      </p>
      <label htmlFor="source">Source Text</label>
      <textarea
        id="source"
        rows={10}
        value={text}
        disabled={busy}
        aria-describedby={error ? "source-error" : "source-count"}
        aria-invalid={error ? true : undefined}
        onChange={(e) => onText(e.target.value)}
      />
      <div className="source-step__tools">
        <span id="source-count" className="review-hint">
          {count === 1 ? "1 character" : `${count.toLocaleString("en")} characters`}
        </span>
        <input
          type="file"
          className="file"
          accept=".md,.txt,text/markdown,text/plain"
          aria-label="Choose a .md or .txt file"
          disabled={busy}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) onFile(file);
            e.target.value = "";
          }}
        />
        <button type="button" className="play-btn play-btn--primary" disabled={busy || !text.trim()} onClick={onDraft}>
          Draft
        </button>
      </div>
      {running ?? <p className="review-hint">A draft takes a few minutes.</p>}
      {error && (
        <p id="source-error" className="source-step__error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
