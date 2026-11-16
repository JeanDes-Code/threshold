import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { DraftView, InsideLine } from "../api";
import { seedDraftView } from "../fixtures/seedDraft";
import SeedReview, { type ReviewClient } from "./SeedReview";

const never = () => Promise.reject(new Error("not called"));
const client: ReviewClient = {
  saveDraft: never,
  confirmAge: never,
  confirmInside: never,
  renameDraftEntity: never,
  approveDraft: never,
};

const rowOf = (name: string) => seedDraftView.seed.entities.findIndex((e) => e.name === name);

/** The review of the fixture draft with these place lines, as static HTML. */
function render(places: InsideLine[], gaps: DraftView["gaps"] = []): string {
  const view: DraftView = { ...seedDraftView, places, gaps };
  return renderToStaticMarkup(
    <SeedReview view={view} client={client} onApproved={() => undefined} onBack={() => undefined} />,
  );
}

/** The "Inside" row of one Place: from its select to the end of its entry. */
function insideRow(html: string, name: string): string {
  const id = `f-entities-${rowOf(name)}--inside`;
  const start = html.indexOf(`id="${id}"`);
  expect(start, id).toBeGreaterThan(-1);
  return html.slice(start, html.indexOf("</li>", start));
}

const selected = (row: string) => /<option value="([^"]*)" selected="">/.exec(row)?.[1];
const confirmDisabled = (row: string) => /<button[^>]*disabled=""[^>]*>Confirm<\/button>/.test(row);

const line = (name: string, more: Partial<InsideLine>): InsideLine => ({
  index: rowOf(name),
  name,
  inside: "unknown",
  doubted: false,
  backed: false,
  confirmed: false,
  suggested: null,
  ...more,
});

describe("the Inside row of the Seed review (ADR 0008, 2026-11-16)", () => {
  it("starts a link no fact backs on “Choose…”, with the model's guess as a hint, and Confirm waits for a pick", () => {
    const html = render([
      line("Ostley Crossing", { suggested: "top" }),
      line("The Copper Kettle", { suggested: { place: "Ostley Crossing" } }),
    ]);
    const kettle = insideRow(html, "The Copper Kettle");
    expect(selected(kettle)).toBe("");
    expect(kettle).toContain("Suggested: inside Ostley Crossing");
    expect(confirmDisabled(kettle)).toBe(true);

    // A guess of "top level" shows no hint: it would lead the Player to
    // the link that makes two Places apart.
    const crossing = insideRow(html, "Ostley Crossing");
    expect(selected(crossing)).toBe("");
    expect(crossing).not.toContain("Suggested:");
    expect(confirmDisabled(crossing)).toBe(true);
  });

  it("shows no hint on a doubted link, whatever the model guessed", () => {
    const html = render([
      line("Ostley Crossing", { suggested: "top" }),
      line("The Copper Kettle", { doubted: true, suggested: { place: "Ostley Crossing" } }),
    ]);
    const kettle = insideRow(html, "The Copper Kettle");
    expect(selected(kettle)).toBe("");
    expect(kettle).toContain("needs a choice");
    expect(kettle).not.toContain("Suggested:");
  });

  it("starts a backed link on the model's choice, with no hint, and Confirm ready", () => {
    const html = render([
      line("Ostley Crossing", { suggested: "top" }),
      line("The Copper Kettle", { inside: { place: "Ostley Crossing" }, backed: true }),
    ]);
    const kettle = insideRow(html, "The Copper Kettle");
    expect(selected(kettle)).toBe("place:Ostley Crossing");
    expect(kettle).not.toContain("Suggested:");
    expect(confirmDisabled(kettle)).toBe(false);
  });

  it("drops the hint once the Player confirmed a link", () => {
    const html = render([
      line("Ostley Crossing", { inside: "unknown", confirmed: true }),
      line("The Copper Kettle", { suggested: { place: "Ostley Crossing" } }),
    ]);
    const crossing = insideRow(html, "Ostley Crossing");
    expect(selected(crossing)).toBe("unknown");
    expect(crossing).not.toContain("Suggested:");
  });

  it("counts the places left in the footer (QA run 4, 307)", () => {
    const html = render([
      line("Ostley Crossing", { inside: "top", confirmed: true }),
      line("The Copper Kettle", { suggested: { place: "Ostley Crossing" } }),
    ], [{ kind: "place_unconfirmed", at: `entities[${rowOf("The Copper Kettle")}].inside` }]);
    const footer = html.slice(html.indexOf('class="review-bar__status"'));
    expect(footer).toMatch(/1 place to confirm/);
  });
});
