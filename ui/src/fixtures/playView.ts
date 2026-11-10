// One invented PlayView for the Play preview and the UI tests. Every name,
// place and line here is made up. It holds each case the screen must draw.

import type { NameRef, Part, PlayView } from "../api";

const maren: NameRef = { entity_id: 2, kind: "npc", name: "Maren" };
const orsel: NameRef = { entity_id: 3, kind: "npc", name: "Orsel" };
const stranger: NameRef = { entity_id: 7, kind: "npc", name: null };
const pc: NameRef = { entity_id: 1, kind: "pc", name: "Ada Quell" };
const guild: NameRef = { entity_id: 5, kind: "faction", name: "the Salt Guild" };
const hale: NameRef = { entity_id: 8, kind: "npc", name: "Captain Hale" };

const text = (t: string): Part => ({ kind: "text", text: t });
const name = (who: NameRef, shown?: string): Part => ({
  kind: "name",
  entity_id: who.entity_id,
  text: shown ?? who.name ?? "",
});
const place = (entityId: number, shown: string): Part => ({ kind: "name", entity_id: entityId, text: shown });

/** The raw Narration of the last Turn as the narrator streams it, speech marks included. */
export const streamText = [
  "Maren turns the ledger round so it faces you, one finger pinned to the last line.",
  "",
  'Maren: "Read it here. It does not leave the counter, and neither do you until I say so."',
  "",
  "The entry is short. The Gull was due two tides ago; someone has struck out the berth number and written nothing in its place. Behind you the door bangs open on the wind, and Orsel shoulders in, salt on his coat.",
  "",
  'Orsel: "Late. Everyone is late tonight. Maren, the Guild wants the night roster."',
  "",
  'From somewhere near the door, someone mutters, "Not with the tide this low." When you look, no one meets your eye.',
].join("\n");

export const playView: PlayView = {
  pc_name: "Ada Quell",
  place: { entity_id: 4, name: "The Harbour Office" },
  clock_min: 2 * 1440 + 17 * 60 + 40,
  needs_begin: false,
  can_correct: true,
  scene: { kind: "plain" },
  story: [
    {
      turn_id: 1,
      kind: "opening",
      intent: "",
      roll: null,
      reverted: false,
      body: {
        kind: "narration",
        blocks: [
          {
            kind: "prose",
            parts: [
              text("Rain runs off the eaves of "),
              place(4, "the Harbour Office"),
              text(
                " in a steady sheet. Inside, the lamps are already lit. A long counter splits the room; behind it ",
              ),
              name(maren),
              text(", the night clerk, sorts dockets into wooden trays without looking up."),
            ],
          },
          {
            kind: "prose",
            parts: [
              text("The sealed manifest in your coat feels heavier than paper should. "),
              name(guild, "The Salt Guild"),
              text(" wants it on this counter before the bell, and the bell is not far off."),
            ],
          },
        ],
      },
    },
    {
      turn_id: 2,
      kind: "play",
      intent: "I hand Maren the sealed manifest and ask whether the Gull has docked.",
      roll: null,
      reverted: false,
      body: {
        kind: "narration",
        blocks: [
          {
            kind: "prose",
            parts: [
              name(maren),
              text(" breaks the seal with her thumbnail and reads the first page twice. Her mouth goes thin."),
            ],
          },
          {
            kind: "speech",
            speaker: maren,
            parts: [
              text("The Gull has not docked. Nobody has seen her since the morning tide, and "),
              name(hale),
              text(" swore to me she would beat the weather. This manifest says she carries forty crates for "),
              name(guild),
              text("."),
            ],
          },
          {
            kind: "prose",
            parts: [
              text("A man in a grey coat by the window lowers his newspaper an inch, then raises it again."),
            ],
          },
        ],
      },
    },
    {
      turn_id: 3,
      kind: "play",
      intent: "I ask who keeps the berth roster at night.",
      roll: null,
      reverted: false,
      body: { kind: "missing" },
    },
    {
      turn_id: 4,
      kind: "play",
      intent: "I climb over the counter to read the ledger myself.",
      roll: { seed: 41, difficulty: 14, value: 6, success: false },
      reverted: true,
      body: {
        kind: "narration",
        blocks: [
          {
            kind: "prose",
            parts: [
              text(
                "You get one knee on the counter before the edge of a tray catches your palm. Maren steps back and reaches for the bell rope.",
              ),
            ],
          },
        ],
      },
    },
    {
      turn_id: 5,
      kind: "correction",
      intent: "I ask Maren to let me read the ledger, politely.",
      roll: { seed: 77, difficulty: 12, value: 15, success: true },
      reverted: false,
      body: {
        kind: "narration",
        blocks: [
          {
            kind: "prose",
            parts: [
              name(maren),
              text(" turns the ledger round so it faces you, one finger pinned to the last line."),
            ],
          },
          {
            kind: "speech",
            speaker: maren,
            parts: [text("Read it here. It does not leave the counter, and neither do you until I say so.")],
          },
          {
            kind: "prose",
            parts: [
              text(
                "The entry is short. The Gull was due two tides ago; someone has struck out the berth number and written nothing in its place. Behind you the door bangs open on the wind, and ",
              ),
              name(orsel),
              text(" shoulders in, salt on his coat."),
            ],
          },
          {
            kind: "speech",
            speaker: orsel,
            parts: [
              text("Late. Everyone is late tonight. "),
              name(maren),
              text(", the Guild wants the night roster."),
            ],
          },
          {
            kind: "prose",
            parts: [
              text(
                "From somewhere near the door, someone mutters, “Not with the tide this low.” When you look, no one meets your eye.",
              ),
            ],
          },
        ],
      },
    },
  ],
  here: [maren, orsel, stranger],
  you: {
    name: "Ada Quell",
    concept: "A courier who owes the Salt Guild more than she can pay.",
    state: [
      { key: "injury", value: "a shallow cut on the left palm" },
      { key: "carrying", value: "an empty satchel and a broken seal" },
      { key: "mood", value: "uneasy" },
    ],
  },
  open: [
    {
      belief: { subject_id: 1, key: "pending_ledger" },
      subject: pc,
      text: "Ada must leave the ledger on the counter before Maren closes the office.",
      open: true,
      told_by: maren,
    },
    {
      belief: { subject_id: 3, key: "pending_roster" },
      subject: orsel,
      text: "Orsel must bring the night roster to the Salt Guild.",
      open: true,
      told_by: orsel,
    },
    {
      belief: { subject_id: 1, key: "pending_debt" },
      subject: pc,
      text: "Ada owes the Salt Guild forty marks by the end of Day 5.",
      open: true,
      told_by: null,
    },
  ],
  pinned: [
    {
      target: { kind: "belief", subject_id: 8, key: "pending_arrival" },
      title: "Captain Hale",
      text: "His ship, the Gull, is two tides late. The berth number in the ledger is struck out.",
    },
    {
      target: { kind: "entity", entity_id: 6 },
      title: "The Lantern Steps",
      text: "Where the night porters wait for work.",
    },
  ],
  known: [
    { who: maren, other_names: ["the night clerk"], here: true },
    { who: orsel, other_names: ["the foreman"], here: true },
    { who: { entity_id: 4, kind: "place", name: "The Harbour Office" }, other_names: [], here: true },
    { who: guild, other_names: ["the Guild"], here: false },
    { who: { entity_id: 6, kind: "place", name: "The Lantern Steps" }, other_names: [], here: false },
    { who: hale, other_names: ["Hale of the Gull"], here: false },
  ],
};
