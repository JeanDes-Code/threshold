import { describe, expect, it } from "vitest";
import type { Note, PlayView, StoryTurn, Target } from "../api";
import { playView } from "../fixtures/playView";
import {
  NOTE_MAX_CHARS,
  currentScene,
  factTarget,
  isPinned,
  isSaveKey,
  noteProblem,
  notesAbout,
  notesNewestFirst,
  sameTarget,
} from "./logic";

const note = (id: number, about: Target | null, created_ms = id): Note => ({
  id,
  body: `note ${id}`,
  about,
  created_ms,
  updated_ms: created_ms,
});

describe("targets", () => {
  it("two targets match on kind and every field", () => {
    expect(sameTarget({ kind: "act", turn_id: 4 }, { kind: "act", turn_id: 4 })).toBe(true);
    expect(sameTarget({ kind: "act", turn_id: 4 }, { kind: "scene", turn_id: 4 })).toBe(false);
    expect(sameTarget({ kind: "act", turn_id: 4 }, { kind: "act", turn_id: 5 })).toBe(false);
    const door = { kind: "belief", subject_id: 2, key: "door" } as const;
    expect(sameTarget(door, { ...door })).toBe(true);
    expect(sameTarget(door, { ...door, key: "gate" })).toBe(false);
    expect(sameTarget(door, { ...door, subject_id: 3 })).toBe(false);
    expect(sameTarget({ kind: "note", note_id: 1 }, { kind: "entity", entity_id: 1 })).toBe(false);
  });

  it("a fact line is pinned by its Belief", () => {
    const line = playView.open[0];
    if (!line) throw new Error("the fixture has open lines");
    const target = factTarget(line);
    expect(target).toEqual({ kind: "belief", subject_id: line.belief.subject_id, key: line.belief.key });
    expect(isPinned([{ kind: "act", turn_id: 1 }, target], target)).toBe(true);
    expect(isPinned([{ kind: "act", turn_id: 1 }], target)).toBe(false);
  });

  it("notes about a target, the latest first", () => {
    const orsel: Target = { kind: "entity", entity_id: 3 };
    const notes = [note(1, orsel, 10), note(2, null, 30), note(3, orsel, 20)];
    expect(notesAbout(notes, orsel).map((n) => n.id)).toEqual([1, 3]);
    expect(notesNewestFirst(notes).map((n) => n.id)).toEqual([2, 3, 1]);
  });
});

describe("note bodies", () => {
  it("an empty or too long body is refused before the call, emoji counted as one", () => {
    expect(noteProblem("")).toBe("Write something first.");
    expect(noteProblem("  \n ")).toBe("Write something first.");
    expect(noteProblem("a".repeat(NOTE_MAX_CHARS))).toBeNull();
    expect(noteProblem("🕯".repeat(NOTE_MAX_CHARS))).toBeNull();
    expect(noteProblem("a".repeat(NOTE_MAX_CHARS + 1))).toBe(
      "A note holds at most 2000 characters. This one has 2001.",
    );
  });

  it("Ctrl+Enter saves, Enter alone does not", () => {
    expect(isSaveKey({ key: "Enter", ctrlKey: true, metaKey: false })).toBe(true);
    expect(isSaveKey({ key: "Enter", ctrlKey: false, metaKey: true })).toBe(true);
    expect(isSaveKey({ key: "Enter", ctrlKey: false, metaKey: false })).toBe(false);
    expect(isSaveKey({ key: "s", ctrlKey: true, metaKey: false })).toBe(false);
  });
});

describe("currentScene", () => {
  const turn = (turn_id: number, extra: Partial<StoryTurn> = {}): StoryTurn => ({
    turn_id,
    kind: "play",
    intent: "",
    roll: null,
    reverted: false,
    body: { kind: "missing" },
    ...extra,
  });

  it("the quick Note is about the Scene of the latest standing Turn", () => {
    const view: PlayView = { ...playView, story: [turn(1, { kind: "opening" }), turn(2), turn(3, { reverted: true })] };
    expect(currentScene(view)).toEqual({ kind: "scene", turn_id: 2 });
  });

  it("before the story begins a Note is free", () => {
    expect(currentScene({ ...playView, story: [] })).toBeNull();
  });
});
