// An invented Seed Draft for the review preview. Every name and fact here is
// made up, after the fixture chronicle of Ostley Crossing.

import type { AdventureSeed, DraftView, Gap, KnownFact } from "../api";

const seed: AdventureSeed = {
  premise:
    "Teodor Brandt runs the last rope ferry at Ostley Crossing. The Bargemen's Guild wants the crossing, and a sealed letter he carries could decide who gets it.",
  pc: {
    name: "Teodor Brandt",
    age: 34,
    other_names: [{ name: "Wick", known_to: ["Agathe Morrow", "Sefa Lind"] }],
    concept: "A careful ferryman who keeps his word and his debts.",
  },
  entities: [
    { name: "Agathe Morrow", kind: "npc", age: 67, other_names: [], hidden: false },
    { name: "Sefa Lind", kind: "npc", age: 19, other_names: [], hidden: false },
    {
      name: "Gerrit Vane",
      kind: "npc",
      age: 52,
      other_names: [{ name: "the Toll-Master", known_to: ["Teodor Brandt", "Agathe Morrow"] }],
      hidden: false,
    },
    { name: "Pim Quast", kind: "npc", age: 11, other_names: [], hidden: false },
    { name: "Ostley Crossing", kind: "place", age: null, other_names: [], hidden: false },
    { name: "The Copper Kettle", kind: "place", age: null, other_names: [], hidden: false },
    { name: "The Bargemen's Guild", kind: "faction", age: null, other_names: [], hidden: false },
  ],
  boundaries: ["No harm to children on screen."],
  decided: [],
  proposed: [],
  story_language: "en",
  themes: ["debt", "trust", "river trade"],
  gm_agreement: "Slow, grounded scenes. The river matters.",
  resolution: "The dice decide uncertain actions.",
  min_age: null,
  start: {
    place: "The Copper Kettle",
    present: ["Agathe Morrow", "Sefa Lind", "Pim"],
    situation: "Teodor comes in wet from the last crossing. Agathe Morrow pours him tea.",
    when: "The evening of the third day of the thaw",
  },
  open_details: [{ text: "Who sealed the letter.", mode: "discover" }],
  detail: "excluded",
};

const knowledge: KnownFact[] = [
  { subject: "Agathe Morrow", key: "trade", value: "innkeeper", text: "Agathe Morrow keeps the Copper Kettle.", open: false },
  { subject: "Gerrit Vane", key: "office", value: "toll-master", text: "Gerrit Vane collects the tolls at the crossing.", open: false },
  { subject: "Teodor Brandt", key: "debt", value: "forty marks", text: "Teodor owes the Guild forty marks.", open: false },
  { subject: "Gerrit Vane", key: "office", value: "guild clerk", text: "Gerrit Vane writes for the Guild.", open: false },
  { subject: "The Bargemen's Guild", key: "vote", value: "buy the ferry", text: "The Guild votes on buying the ferry at the spring meeting.", open: true },
  { subject: "Sefa Lind", key: "letter", value: "Harrowmere", text: "Sefa Lind wants the letter carried to Harrowmere.", open: true },
];

/** The age each character had in the draft, and whether the model says the text states it. */
export const draftedAges: Record<string, { age: number; stated: boolean }> = {
  "Teodor Brandt": { age: 34, stated: true },
  "Agathe Morrow": { age: 67, stated: true },
  "Sefa Lind": { age: 19, stated: false },
  "Gerrit Vane": { age: 50, stated: false },
  "Pim Quast": { age: 11, stated: true },
};

/** The ages the Player already confirmed in the preview. */
export const confirmedAges: Record<string, number> = { "Agathe Morrow": 67, "Pim Quast": 11 };

/** The gaps that are not about ages; the preview derives the age gaps. */
export const otherGaps: Gap[] = [
  { kind: "unknown_name", at: "start.present[2]" },
  { kind: "slot_conflict", at: "knowledge[3]" },
];

export const seedDraftView: DraftView = {
  draft_id: "draft-1",
  state: "draft",
  seed,
  knowledge,
  ages: [],
  cut: [{ at: "entities", cap: 30 }],
  gaps: [],
  last_failure: null,
};

/** A Source Text far too short to hold a premise and a PC. */
export const shortSource = "A ferryman owes the Guild forty marks.";
