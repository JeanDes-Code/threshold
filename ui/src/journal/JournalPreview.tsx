// Dev only: the Journal on the invented fixture, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=journal while `npm run dev` runs.
// `&part=decisions`, `open`, `before` or `notes` opens that part.

import { journalView } from "../fixtures/journalView";
import Journal, { PARTS, type JournalPart } from "./Journal";

function initialPart(): JournalPart {
  const asked = new URLSearchParams(window.location.search).get("part");
  return PARTS.find((p) => p.id === asked)?.id ?? "timeline";
}

export default function JournalPreview() {
  return (
    <div className="preview">
      <div className="preview__strip">
        <span className="preview__label">Preview · fixture data</span>
      </div>
      <Journal view={journalView} pcName="Ada Quell" initialPart={initialPart()} />
    </div>
  );
}
