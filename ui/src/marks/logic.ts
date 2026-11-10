// Pure rules of Notes and Pins on the screens. The core stores them, checks
// each body again and says where a target stands now; these only compare and
// pick what to send.

import type { FactLine, Note, PlayView, Target } from "../api";

/** The longest Note body, in characters, as the core counts them (an emoji is one). */
export const NOTE_MAX_CHARS = 2000;

/** True when two targets name the same thing. */
export function sameTarget(a: Target, b: Target): boolean {
  switch (a.kind) {
    case "scene":
    case "act":
      return b.kind === a.kind && b.turn_id === a.turn_id;
    case "belief":
      return b.kind === "belief" && b.subject_id === a.subject_id && b.key === a.key;
    case "entity":
      return b.kind === "entity" && b.entity_id === a.entity_id;
    case "note":
      return b.kind === "note" && b.note_id === a.note_id;
  }
}

export function isPinned(pins: Target[], target: Target): boolean {
  return pins.some((pin) => sameTarget(pin, target));
}

/** The target of a fact line: the Belief it shows. */
export function factTarget(line: FactLine): Target {
  return { kind: "belief", subject_id: line.belief.subject_id, key: line.belief.key };
}

/** The Notes about `target`, as the core resolved them. */
export function notesAbout(notes: Note[], target: Target): Note[] {
  return notes.filter((note) => note.about !== null && sameTarget(note.about, target));
}

/** The latest Note first. */
export function notesNewestFirst(notes: Note[]): Note[] {
  return [...notes].sort((a, b) => b.created_ms - a.created_ms || b.id - a.id);
}

/** A body's length as the core counts it: characters of the trimmed text. */
export function noteLength(body: string): number {
  return [...body.trim()].length;
}

/** Why the body cannot be saved yet, or null. The core checks it again. */
export function noteProblem(body: string): string | null {
  const length = noteLength(body);
  if (length === 0) return "Write something first.";
  if (length > NOTE_MAX_CHARS) return `A note holds at most ${NOTE_MAX_CHARS} characters. This one has ${length}.`;
  return null;
}

/** Ctrl+Enter (or Cmd+Enter) saves a Note; Enter alone makes a new line. */
export function isSaveKey(e: { key: string; ctrlKey: boolean; metaKey: boolean }): boolean {
  return e.key === "Enter" && (e.ctrlKey || e.metaKey);
}

/**
 * The Scene the Play screen shows now: the Scene of the latest Turn that
 * stands. The core resolves a Scene target from any of its Turns. Null
 * before the story begins.
 */
export function currentScene(view: PlayView): Target | null {
  const standing = view.story.filter((t) => !t.reverted && t.kind !== "setup" && t.kind !== "ooc");
  const turn = standing[standing.length - 1];
  return turn ? { kind: "scene", turn_id: turn.turn_id } : null;
}
