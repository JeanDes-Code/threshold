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
});
