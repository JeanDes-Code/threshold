// Pure rules of the Play screen. The components hold no logic of their own.

import {
  BEGIN_OP_ID,
  renarrateOpId,
  type DetailView,
  type KnownLine,
  type NameRef,
  type PlayView,
  type SceneState,
  type StoryTurn,
} from "../api";

/**
 * "answering": the GM answers an out-of-character question; no Turn runs.
 * "fading": the Player faded the Narration; the call that ends it runs.
 */
export type TurnPhase = "idle" | "deciding" | "narrating" | "answering" | "fading";

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

/**
 * The GM decides until the first chunk arrives, then narrates. A busy
 * screen with no stream at all is an out-of-character question.
 */
export function turnPhase(busy: boolean, stream: string | null): TurnPhase {
  if (!busy) return "idle";
  if (stream === null) return "answering";
  return stream === "" ? "deciding" : "narrating";
}

/** The words of the action bar for each phase. */
export const PHASE_TEXT: Record<TurnPhase, string> = {
  idle: "",
  deciding: "The GM decides",
  narrating: "The GM narrates",
  answering: "The GM answers out of character",
  fading: "Fading to black",
};

/** "Correct last Turn" opens only on a standing Play Turn, and never while a Turn runs. */
export function canOpenCorrection(canCorrect: boolean, busy: boolean): boolean {
  return canCorrect && !busy;
}

/** The stream as the story shows it: a quiet "…" until the first chunk. */
export function streamShown(text: string): { text: string; waiting: boolean } {
  return text === "" ? { text: "…", waiting: true } : { text, waiting: false };
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

/**
 * The Intent field after a Turn ends. The field clears when the Intent is
 * sent, and the Player may type the next one while the Turn runs: that text
 * stays. When the Turn was not taken, the sent text comes back; if the
 * Player typed something meanwhile, the field holds both: the sent text, a
 * new line, then what was typed.
 */
export function intentAfterTurn(sent: string, typed: string, taken: boolean): string {
  if (taken) return typed;
  if (typed.trim() === "") return sent;
  return `${sent}\n${typed}`;
}

/**
 * The Intent field after a Retry commits. The failed Turn gave its sent text
 * back to the field; once the Retry commits that text is done, so it goes,
 * and only what the Player typed after it stays (QA, bug pass 4: the next
 * Intent was glued to the retried one).
 */
export function intentAfterRetry(retried: string, typed: string): string {
  if (retried === "") return typed;
  if (typed.trim() === retried.trim()) return "";
  if (typed.startsWith(`${retried}\n`)) return typed.slice(retried.length + 1);
  return typed;
}

/** What the Player sent last from the action bar: an Intent or a Correction. */
export type Sent = "intent" | "correction";

/**
 * The action bar after a Retry commits (review minor). A retried Intent
 * takes its text out of the field (`intentAfterRetry`). A retried
 * Correction closes the Fix form and clears it, and leaves the Intent field
 * alone: text that an older failed Intent gave back was never played.
 */
export function afterRetry(sent: Sent, returned: string, typed: string): { intent: string; closeFix: boolean } {
  return sent === "correction"
    ? { intent: typed, closeFix: true }
    : { intent: intentAfterRetry(returned, typed), closeFix: false };
}

/** The ms a call has run since `startedMs`; 0 when none runs. */
export function elapsedSince(startedMs: number | null, now: number): number {
  return startedMs === null ? 0 : Math.max(0, now - startedMs);
}

/** The seconds a running Turn has taken, as the phase line shows them. */
export function elapsedText(ms: number): string {
  return `${Math.max(0, Math.floor(ms / 1000))} s`;
}

/** How many Open Items the side panel shows before "Show all". */
export const OPEN_CAP = 5;

/**
 * The Open Items the side panel shows: the first `OPEN_CAP`, or all. The
 * core sends them in play order (those about someone here first, then the
 * newest), so the first ones are the ones that matter now.
 */
export function openShown<T>(lines: T[], all: boolean): T[] {
  return all ? lines : lines.slice(0, OPEN_CAP);
}

/**
 * A Narration's text cut at `*…*` marks: the words between a pair are
 * emphasis, and any other asterisk goes. The Player never reads a raw
 * asterisk.
 */
export function emphasis(text: string): { text: string; em: boolean }[] {
  const out: { text: string; em: boolean }[] = [];
  const push = (part: string, em: boolean) => {
    if (part !== "") out.push({ text: part, em });
  };
  let rest = text;
  for (;;) {
    // As in Markdown: a pair holds words with no blank right inside it.
    const match = /\*+([^\s*](?:[^*\n]*[^\s*])?)\*+/.exec(rest);
    if (!match) break;
    push(rest.slice(0, match.index).replaceAll("*", ""), false);
    push(match[1] ?? "", true);
    rest = rest.slice(match.index + match[0].length);
  }
  push(rest.replaceAll("*", ""), false);
  return out.length === 0 ? [{ text: "", em: false }] : out;
}

/**
 * The Turn whose name links are in the tab order: the last committed Turn,
 * while no stream runs. Older names stay links for the mouse, so the
 * keyboard reaches the side panel without walking the whole story.
 */
export function tabbableTurn(items: StoryItem[]): number | null {
  const last = items.at(-1);
  return last?.kind === "turn" ? last.turn.turn_id : null;
}


/** One button of the action bar's right slot. */
export interface LaterButton {
  shown: boolean;
  enabled: boolean;
}

/**
 * The buttons of the mature content module in the action bar's right slot
 * (data shape 3.12), from the scene and the phase only: the core sends no
 * list of actions. "End scene" shows in an Intimate Scene and works while
 * no call runs. "Fade to black" shows while a Narration streams or in an
 * Intimate Scene, and works while a Narration streams that can be faded
 * (`fadeable`) or, in a scene, while no call runs.
 */
export function laterButtons(
  scene: SceneState,
  phase: TurnPhase,
  canFade = true,
): { end: LaterButton; fade: LaterButton } {
  const intimate = scene.kind === "intimate";
  const narrating = phase === "narrating";
  return {
    end: { shown: intimate, enabled: intimate && phase === "idle" },
    fade: { shown: narrating || intimate, enabled: (narrating && canFade) || (intimate && phase === "idle") },
  };
}

/**
 * True when the Narration that streams under `opId` can be faded. The
 * Opening's never can (data shape 10): Begin, and a renarration of the
 * Opening Turn, read no fade flag, so the button would do nothing there.
 * With no stream (`null`) or any other op, true.
 */
export function fadeable(opId: string | null, story: StoryTurn[]): boolean {
  if (opId === BEGIN_OP_ID) return false;
  const opening = story.find((turn) => turn.kind === "opening");
  return opening === undefined || opId !== renarrateOpId(opening.turn_id);
}

/**
 * The notice of data shape 3.11: the Campaign asks for explicit description
 * and no loaded Content Pack gives it, so scenes are told by implication.
 */
export const IMPLIED_NOTICE = "No loaded Content Pack gives explicit description. Scenes are told by implication.";

/**
 * The implied notice, once per session: it shows on the Play screen while
 * the stored level is `explicit` and the effective level is `implied`, until
 * the Player closes it; once closed, no view of the session shows it again,
 * in this Campaign or another.
 */
export function impliedNotice(detail: DetailView, closed: boolean): string | null {
  const due = detail.stored === "explicit" && detail.effective === "implied";
  return due && !closed ? IMPLIED_NOTICE : null;
}
