import { describe, expect, it } from "vitest";
import type { Block, Part, PlayView } from "../api";
import { playView, streamText } from "./playView";

const blocksOf = (view: PlayView): Block[] =>
  view.story.flatMap((turn) => (turn.body.kind === "narration" ? turn.body.blocks : []));

const textOf = (parts: Part[]): string => parts.map((p) => p.text).join("");

describe("the Play fixture", () => {
  it("the fixture holds every case the owner must see", () => {
    const blocks = blocksOf(playView);
    expect(blocks.some((b) => b.kind === "speech")).toBe(true);
    // The unsure line: a quote with no certain speaker stays in the prose.
    expect(blocks.some((b) => b.kind === "prose" && /["“«]/.test(textOf(b.parts)))).toBe(true);
    expect(playView.here.some((who) => who.name === null)).toBe(true);
    expect(playView.open.length).toBeGreaterThanOrEqual(2);
    expect(playView.pinned.length).toBeGreaterThanOrEqual(1);
    expect(playView.story.some((turn) => turn.reverted)).toBe(true);
    expect(playView.story.some((turn) => turn.roll !== null)).toBe(true);
    expect(playView.story.some((turn) => turn.body.kind === "missing")).toBe(true);
    expect(streamText).toMatch(/^[A-Z][\p{L} ]*: ["“«]/mu);
  });

  it("every name part points at a known entity", () => {
    // YouCard holds no entity id, so the PC is never a name link.
    const ids = new Set([
      ...playView.known.map((line) => line.who.entity_id),
      ...playView.here.map((who) => who.entity_id),
    ]);
    const parts = blocksOf(playView).flatMap((b) => b.parts);
    const names = parts.filter((p) => p.kind === "name");
    expect(names.length).toBeGreaterThan(0);
    for (const part of names) expect(ids.has(part.entity_id)).toBe(true);
  });
});
