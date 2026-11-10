// Dev only: the Journal on the invented fixture, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=journal while `npm run dev` runs.
// `&part=decisions`, `open`, `before` or `notes` opens that part. Notes and
// Pins change in memory only.

import { useState } from "react";
import type { JournalView } from "../api";
import { journalView } from "../fixtures/journalView";
import { sameTarget } from "../marks/logic";
import Journal, { PARTS, type JournalActions, type JournalPart } from "./Journal";

function initialPart(): JournalPart {
  const asked = new URLSearchParams(window.location.search).get("part");
  return PARTS.find((p) => p.id === asked)?.id ?? "timeline";
}

export default function JournalPreview() {
  const [view, setView] = useState<JournalView>(journalView);
  const now = () => Date.now();

  const actions: JournalActions = {
    onPin: (target, on) =>
      setView((v) => ({
        ...v,
        pins: on ? [...v.pins, target] : v.pins.filter((p) => !sameTarget(p, target)),
      })),
    onAddNote: async (body, about) => {
      setView((v) => {
        const id = Math.max(0, ...v.notes.map((n) => n.id)) + 1;
        const note = { id, body: body.trim(), about, created_ms: now(), updated_ms: now() };
        return { ...v, notes: [...v.notes, note] };
      });
      return null;
    },
    onEditNote: async (noteId, body) => {
      setView((v) => ({
        ...v,
        notes: v.notes.map((n) => (n.id === noteId ? { ...n, body: body.trim(), updated_ms: now() } : n)),
      }));
      return null;
    },
    onDeleteNote: async (noteId) => {
      setView((v) => ({
        ...v,
        notes: v.notes.filter((n) => n.id !== noteId),
        pins: v.pins.filter((p) => !sameTarget(p, { kind: "note", note_id: noteId })),
      }));
      return null;
    },
  };

  return (
    <div className="preview">
      <div className="preview__strip">
        <span className="preview__label">Preview · fixture data</span>
      </div>
      <Journal view={view} pcName="Ada Quell" actions={actions} initialPart={initialPart()} />
    </div>
  );
}
