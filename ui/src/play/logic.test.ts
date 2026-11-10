import { describe, expect, it } from "vitest";
import { playView } from "../fixtures/playView";
import {
  isSendKey,
  matchesSearch,
  nameText,
  npcColor,
  shouldFollow,
  storyItems,
  turnPhase,
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

  it("the people search reads every name the PC knows, case and accents aside", () => {
    const line = {
      who: { entity_id: 4, kind: "npc" as const, name: "Chloé Varn" },
      other_names: ["the Tallyman"],
      here: false,
    };
    expect(matchesSearch(line, "")).toBe(true);
    expect(matchesSearch(line, "chloe")).toBe(true);
    expect(matchesSearch(line, "  TALLY ")).toBe(true);
    expect(matchesSearch(line, "maren")).toBe(false);
  });

  it("Enter sends and Shift+Enter does not", () => {
    expect(isSendKey({ key: "Enter", shiftKey: false })).toBe(true);
    expect(isSendKey({ key: "Enter", shiftKey: true })).toBe(false);
    expect(isSendKey({ key: "a", shiftKey: false })).toBe(false);
  });
});
