import { describe, expect, it } from "vitest";
import type { KnownLine } from "../api";
import { playView } from "../fixtures/playView";
import {
  filterKnown,
  isSendKey,
  nameText,
  npcColor,
  searchFold,
  shouldFollow,
  storyItems,
  turnPhase,
  PHASE_TEXT,
  canOpenCorrection,
  streamShown,
  intentAfterTurn,
  intentAfterRetry,
  afterRetry,
  elapsedSince,
  elapsedText,
  openShown,
  OPEN_CAP,
  emphasis,
  tabbableTurn,
  laterButtons,
  impliedNotice,
  IMPLIED_NOTICE,
  type TurnPhase,
} from "./logic";

describe("play logic", () => {
  it("npcColor gives each NPC one stable colour", () => {
    expect(npcColor(7)).toBe(npcColor(7));
    const colours = new Set([1, 2, 3, 4, 5, 6, 7, 8].map(npcColor));
    expect(colours.size).toBe(8);
    expect(npcColor(9)).toBe(npcColor(1));
    for (const c of colours) expect(c).toMatch(/^var\(--npc-[1-8]\)$/);
  });

  it("an unnamed character reads as unknown", () => {
    expect(nameText({ entity_id: 9, kind: "npc", name: null })).toBe("unknown");
    expect(nameText({ entity_id: 9, kind: "npc", name: "Orsel" })).toBe("Orsel");
  });

  it("the stream shows raw text until the Turn commits", () => {
    const streaming = storyItems(playView, 'Orsel: "Late."');
    expect(streaming.at(-1)).toEqual({ kind: "stream", text: 'Orsel: "Late."' });
    expect(streaming).toHaveLength(playView.story.length + 1);

    const settled = storyItems(playView, null);
    expect(settled.some((item) => item.kind === "stream")).toBe(false);
    expect(settled).toEqual(playView.story.map((turn) => ({ kind: "turn", turn })));
  });

  it("the bar says deciding, then narrating", () => {
    expect(turnPhase(true, "")).toBe("deciding");
    expect(turnPhase(true, "x")).toBe("narrating");
    expect(turnPhase(false, "")).toBe("idle");
  });

  it("the story follows the stream unless the Player scrolled up", () => {
    expect(shouldFollow(976, 24, 1000)).toBe(true);
    expect(shouldFollow(500, 24, 1000)).toBe(false);
  });

  it("filterKnown matches main and other names, case and accents aside", () => {
    const chloe: KnownLine = {
      who: { entity_id: 4, kind: "npc", name: "Chloé Varn" },
      other_names: ["the Tallyman"],
      here: false,
    };
    const mill: KnownLine = {
      who: { entity_id: 5, kind: "place", name: "Old Mill" },
      other_names: [],
      here: true,
    };
    const lines = [chloe, mill];
    expect(searchFold("CHLOÉ")).toBe("chloe");
    expect(filterKnown(lines, "  Chloé ")).toEqual([chloe]);
    expect(filterKnown(lines, "")).toEqual(lines);
    expect(filterKnown(lines, "   ")).toEqual(lines);
    expect(filterKnown(lines, "CHLOE")).toEqual([chloe]);
    expect(filterKnown(lines, "varn")).toEqual([chloe]);
    expect(filterKnown(lines, "tally")).toEqual([chloe]);
    expect(filterKnown(lines, "mill")).toEqual([mill]);
    expect(filterKnown(lines, "maren")).toEqual([]);
  });

  it("Enter sends and Shift+Enter does not", () => {
    expect(isSendKey({ key: "Enter", shiftKey: false })).toBe(true);
    expect(isSendKey({ key: "Enter", shiftKey: true })).toBe(false);
    expect(isSendKey({ key: "a", shiftKey: false })).toBe(false);
  });
});

describe("Task 5 fixes", () => {
  it("an OOC question says the GM answers out of character, not that it decides", () => {
    expect(turnPhase(true, null)).toBe("answering");
    expect(PHASE_TEXT[turnPhase(true, null)]).toBe("The GM answers out of character");
    expect(PHASE_TEXT[turnPhase(true, "")]).toBe("The GM decides");
    expect(turnPhase(false, null)).toBe("idle");
  });

  it("Correct last Turn cannot open while a Turn runs", () => {
    expect(canOpenCorrection(true, false)).toBe(true);
    expect(canOpenCorrection(true, true)).toBe(false);
    expect(canOpenCorrection(false, false)).toBe(false);
  });

  it("before the first chunk the story shows a quiet placeholder", () => {
    expect(streamShown("")).toEqual({ text: "…", waiting: true });
    expect(streamShown("Orsel")).toEqual({ text: "Orsel", waiting: false });
  });

  it("the Intent field clears on send and keeps what the Player types during the Turn", () => {
    // Taken: the field keeps what was typed meanwhile, empty or not.
    expect(intentAfterTurn("I wait.", "", true)).toBe("");
    expect(intentAfterTurn("I wait.", "I look", true)).toBe("I look");
    // Not taken: the sent text comes back; with text typed meanwhile, both.
    expect(intentAfterTurn("I wait.", "", false)).toBe("I wait.");
    expect(intentAfterTurn("I wait.", "  ", false)).toBe("I wait.");
    expect(intentAfterTurn("I wait.", "I look", false)).toBe("I wait.\nI look");
  });

  it("a Retry that commits takes the retried text out of the field", () => {
    expect(intentAfterRetry("I wait.", "I wait.")).toBe("");
    expect(intentAfterRetry("I wait.", "I wait.\nI look")).toBe("I look");
    // The Player rewrote the field: it stays as it is.
    expect(intentAfterRetry("I wait.", "I run.")).toBe("I run.");
    expect(intentAfterRetry("", "I look")).toBe("I look");
  });

  it("a Retry that commits a Correction closes the Fix form and leaves the Intent field alone", () => {
    expect(afterRetry("correction", "I wait.", "I look")).toEqual({ intent: "I look", closeFix: true });
    expect(afterRetry("intent", "I wait.", "I wait.\nI look")).toEqual({ intent: "I look", closeFix: false });
  });

  it("the seconds count from the Turn's start, so a tab switch does not reset them", () => {
    expect(elapsedSince(null, 50_000)).toBe(0);
    expect(elapsedSince(40_000, 50_000)).toBe(10_000);
    expect(elapsedSince(60_000, 50_000)).toBe(0);
  });

  it("a running Turn shows its whole seconds", () => {
    expect(elapsedText(0)).toBe("0 s");
    expect(elapsedText(4999)).toBe("4 s");
    expect(elapsedText(-5)).toBe("0 s");
  });

  it("the Open card shows the first items and the rest on demand", () => {
    const lines = Array.from({ length: 9 }, (_, i) => i);
    expect(OPEN_CAP).toBe(5);
    expect(openShown(lines, false)).toEqual([0, 1, 2, 3, 4]);
    expect(openShown(lines, true)).toEqual(lines);
    expect(openShown(lines.slice(0, 3), false)).toEqual([0, 1, 2]);
  });

  it("asterisks in a Narration become emphasis or go", () => {
    expect(emphasis("The blade goes *swish-swish* past you.")).toEqual([
      { text: "The blade goes ", em: false },
      { text: "swish-swish", em: true },
      { text: " past you.", em: false },
    ]);
    expect(emphasis("A lone * star and **loud**.")).toEqual([
      { text: "A lone  star and ", em: false },
      { text: "loud", em: true },
      { text: ".", em: false },
    ]);
    expect(emphasis("No marks.")).toEqual([{ text: "No marks.", em: false }]);
  });

  it("only the last Turn's names are in the tab order", () => {
    const items = storyItems(playView, null);
    const turnsOnly = items.filter((i) => i.kind === "turn");
    const last = turnsOnly.at(-1);
    expect(last && last.kind === "turn" && tabbableTurn(items)).toBe(last && last.kind === "turn" ? last.turn.turn_id : null);
    expect(tabbableTurn(storyItems(playView, "Orsel"))).toBe(null);
  });

  it("End scene shows in an intimate scene and is enabled only when idle", () => {
    const intimate = { kind: "intimate", since_turn: 7 } as const;
    const phases: TurnPhase[] = ["idle", "deciding", "narrating", "answering"];
    for (const phase of phases) {
      const plain = laterButtons({ kind: "plain" }, phase);
      expect(plain.end).toEqual({ shown: false, enabled: false });
      const scene = laterButtons(intimate, phase);
      expect(scene.end).toEqual({ shown: true, enabled: phase === "idle" });
    }
  });

  it("Fade to black shows while narrating or in a scene, and is enabled as shape 3.12 says", () => {
    const intimate = { kind: "intimate", since_turn: 7 } as const;
    const plain = { kind: "plain" } as const;
    expect(laterButtons(plain, "narrating").fade).toEqual({ shown: true, enabled: true });
    expect(laterButtons(intimate, "narrating").fade).toEqual({ shown: true, enabled: true });
    expect(laterButtons(intimate, "idle").fade).toEqual({ shown: true, enabled: true });
    for (const phase of ["deciding", "answering", "fading"] as TurnPhase[]) {
      expect(laterButtons(intimate, phase).fade).toEqual({ shown: true, enabled: false });
      expect(laterButtons(plain, phase).fade).toEqual({ shown: false, enabled: false });
    }
    expect(laterButtons(plain, "idle").fade).toEqual({ shown: false, enabled: false });
    expect(PHASE_TEXT.fading).not.toBe("");
  });

  it("the implied notice shows once per session", () => {
    const fallback = { stored: "explicit", effective: "implied" } as const;
    // Every Play view that falls back shows it until the Player closes it.
    expect(impliedNotice(fallback, false)).toBe(IMPLIED_NOTICE);
    // Once closed, no later view of the session shows it, in any Campaign.
    expect(impliedNotice(fallback, true)).toBeNull();
    // No notice while explicit description is in use, or never asked for.
    expect(impliedNotice({ stored: "explicit", effective: "explicit" }, false)).toBeNull();
    expect(impliedNotice({ stored: "implied", effective: "implied" }, false)).toBeNull();
    expect(impliedNotice({ stored: "excluded", effective: "excluded" }, false)).toBeNull();
    expect(IMPLIED_NOTICE).not.toMatch(/\d/);
  });
});
