// Pure rules of the Play screen. The components hold no logic of their own.

import type { KnownLine, NameRef, PlayView, StoryTurn } from "../api";

export type TurnPhase = "idle" | "deciding" | "narrating";

export type StoryItem = { kind: "turn"; turn: StoryTurn } | { kind: "stream"; text: string };

/** Within this distance of the bottom, the story follows the stream. */
const FOLLOW_PX = 24;

/** The colour of an NPC: one of eight tokens, stable for its entity id. */
export function npcColor(entityId: number): string {
  return `var(--npc-${(entityId % 8) + 1})`;
}

/** The name the PC knows, or "unknown". */
export function nameText(who: NameRef): string {
  return who.name ?? "unknown";
}

/** The GM decides until the first chunk arrives, then narrates. */
export function turnPhase(busy: boolean, stream: string): TurnPhase {
  if (!busy) return "idle";
  return stream === "" ? "deciding" : "narrating";
}

/** True when the Player has not scrolled up from the bottom of the story. */
export function shouldFollow(scrollTop: number, clientHeight: number, scrollHeight: number): boolean {
  return scrollHeight - (scrollTop + clientHeight) <= FOLLOW_PX;
}

/** Enter sends the Intent; Shift+Enter makes a new line. */
export function isSendKey(e: { key: string; shiftKey: boolean }): boolean {
  return e.key === "Enter" && !e.shiftKey;
}

/** Text for the people search: lower case, accents dropped. */
export function searchFold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/**
 * The lines of "People and places" whose main name or other name holds the
 * search text. It reads the panel's own list, never the story text.
 */
export function filterKnown(lines: KnownLine[], query: string): KnownLine[] {
  const q = searchFold(query).trim();
  if (q === "") return lines;
  return lines.filter((line) =>
    [line.who.name ?? "", ...line.other_names].some((n) => searchFold(n).includes(q)),
  );
}

/** The committed Turns, then the raw stream text while a Narration streams. */
export function storyItems(view: PlayView, stream: string | null): StoryItem[] {
  const items: StoryItem[] = view.story.map((turn) => ({ kind: "turn", turn }));
  if (stream !== null) items.push({ kind: "stream", text: stream });
  return items;
}
