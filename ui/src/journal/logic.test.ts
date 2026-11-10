import { describe, expect, it } from "vitest";
import type { FactLine, JournalView, NameRef } from "../api";
import { journalView } from "../fixtures/journalView";
import { aboutText, bySubject, clockSpan, decisions, placeTitle, sceneOfTurn, scenesNewestFirst } from "./logic";

const maren: NameRef = { entity_id: 2, kind: "npc", name: "Maren" };
const pc: NameRef = { entity_id: 1, kind: "pc", name: "Ada" };

const line = (subject: NameRef, key: string, text: string): FactLine => ({
  belief: { subject_id: subject.entity_id, key },
  subject,
  text,
  open: false,
  told_by: null,
});

const view: JournalView = {
  scenes: [
    {
      turn_id: 3,
      place: "The Harbour Office",
      clock_from: 60,
      clock_to: 95,
      present: [maren],
      acts: [
        { turn_id: 4, intent: "I hand over the manifest.", decision: true },
        { turn_id: 5, intent: "I wait.", decision: false },
      ],
      learned: [],
    },
    {
      turn_id: 6,
      place: "The Lantern Steps",
      clock_from: 130,
      clock_to: 130,
      present: [],
      acts: [{ turn_id: 6, intent: "I walk to the steps.", decision: true }],
      learned: [],
    },
  ],
  open: [],
  closed: [],
  known_before: [],
  notes: [],
  pins: [],
};

describe("journal logic", () => {
  it("the Decisions tab lists decision acts with their scene's place and time", () => {
    const got = decisions(view);
    expect(got.map((d) => d.act.intent)).toEqual(["I walk to the steps.", "I hand over the manifest."]);
    expect(got[0]?.scene.place).toBe("The Lantern Steps");
    expect(got[1]?.scene.place).toBe("The Harbour Office");
    expect(clockSpan(got[1]?.scene.clock_from ?? 0, got[1]?.scene.clock_to ?? 0)).toBe(
      "Day 1, 01:00 – 01:35",
    );
    // An act that is not a decision is not listed.
    expect(got.some((d) => d.act.intent === "I wait.")).toBe(false);
    // The fixture holds decisions too, each in a scene of its own view.
    for (const d of decisions(journalView)) expect(journalView.scenes).toContain(d.scene);
  });

  it("the timeline shows the latest scene first, numbered from the first", () => {
    expect(scenesNewestFirst(view).map(({ scene, number }) => [scene.turn_id, number])).toEqual([
      [6, 2],
      [3, 1],
    ]);
  });

  it("a closed item finds the scene of the Turn that closed it", () => {
    expect(sceneOfTurn(view, 5)?.number).toBe(1);
    expect(sceneOfTurn(view, 3)?.number).toBe(1);
    expect(sceneOfTurn(view, 6)?.scene.place).toBe("The Lantern Steps");
    expect(sceneOfTurn(view, 99)).toBeNull();
  });

  it("a time span reads in words, one day or two", () => {
    expect(clockSpan(60, 60)).toBe("Day 1, 01:00");
    expect(clockSpan(60, 95)).toBe("Day 1, 01:00 – 01:35");
    expect(clockSpan(1380, 1500)).toBe("Day 1, 23:00 – Day 2, 01:00");
  });

  it("a place reads as a title", () => {
    expect(placeTitle("the Guild cellar door")).toBe("The Guild cellar door");
    expect(placeTitle("The Harbour Office")).toBe("The Harbour Office");
    expect(placeTitle("  ")).toBe("Somewhere");
  });

  it("known facts group by subject, first seen first", () => {
    const groups = bySubject([
      line(pc, "role", "Ada is a courier."),
      line(maren, "role", "Maren keeps the night desk."),
      line(pc, "pending_debt", "Ada owes the Guild."),
    ]);
    expect(groups.map((g) => [g.subject.name, g.lines.length])).toEqual([
      ["Ada", 2],
      ["Maren", 1],
    ]);
  });
});

describe("aboutText", () => {
  it("names what a Note is about in words", () => {
    const fixture = journalView;
    const scene = fixture.scenes[1];
    const act = scene?.acts[0];
    const fact = fixture.known_before[0];
    if (!scene || !act || !fact) throw new Error("the fixture has scenes, acts and facts");
    expect(aboutText(fixture, null)).toBe("Free note");
    expect(aboutText(fixture, { kind: "scene", turn_id: scene.turn_id })).toBe(
      `Scene 2 · ${placeTitle(scene.place)}`,
    );
    expect(aboutText(fixture, { kind: "act", turn_id: act.turn_id })).toBe(`What you did: ${act.intent}`);
    expect(aboutText(fixture, { kind: "belief", ...fact.belief })).toBe(`Fact: ${fact.text}`);
    expect(aboutText(fixture, { kind: "entity", entity_id: 2 })).toBe("About Maren");
    expect(aboutText(fixture, { kind: "entity", entity_id: 7 })).toBe("About someone you know");
    expect(aboutText(fixture, { kind: "scene", turn_id: 9999 })).toBe("A scene");
  });
});
