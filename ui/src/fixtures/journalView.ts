// One invented JournalView for the Journal preview and the UI tests. It goes
// on from the story of the Play fixture. Every name and line is made up.

import type { FactLine, JournalView, NameRef } from "../api";

const pc: NameRef = { entity_id: 1, kind: "pc", name: "Ada Quell" };
const maren: NameRef = { entity_id: 2, kind: "npc", name: "Maren" };
const orsel: NameRef = { entity_id: 3, kind: "npc", name: "Orsel" };
const office: NameRef = { entity_id: 4, kind: "place", name: "The Harbour Office" };
const guild: NameRef = { entity_id: 5, kind: "faction", name: "the Salt Guild" };
const steps: NameRef = { entity_id: 6, kind: "place", name: "The Lantern Steps" };
const stranger: NameRef = { entity_id: 7, kind: "npc", name: null };
const hale: NameRef = { entity_id: 8, kind: "npc", name: "Captain Hale" };
const tovin: NameRef = { entity_id: 9, kind: "npc", name: "Tovin" };

const at = (day: number, hh: number, mm: number): number => (day - 1) * 1440 + hh * 60 + mm;

const fact = (subject: NameRef, key: string, text: string, told_by: NameRef | null = null): FactLine => ({
  belief: { subject_id: subject.entity_id, key },
  subject,
  text,
  open: key.startsWith("pending_"),
  told_by,
});

const ledger = fact(
  pc,
  "pending_ledger",
  "Ada must leave the ledger on the counter before Maren closes the office.",
  maren,
);
const roster = fact(orsel, "pending_roster", "Orsel must bring the night roster to the Salt Guild.", orsel);
const arrival = fact(hale, "pending_arrival", "Captain Hale swore the Gull would beat the weather.", maren);
const debt = fact(pc, "pending_debt", "Ada owes the Salt Guild forty marks by the end of Day 5.");

export const journalView: JournalView = {
  scenes: [
    {
      turn_id: 1,
      place: "The Harbour Office",
      clock_from: at(3, 17, 5),
      clock_to: at(3, 17, 40),
      present: [maren, orsel, stranger],
      acts: [
        {
          turn_id: 2,
          intent: "I hand Maren the sealed manifest and ask whether the Gull has docked.",
          decision: true,
        },
        { turn_id: 3, intent: "I ask who keeps the berth roster at night.", decision: false },
        { turn_id: 5, intent: "I ask Maren to let me read the ledger, politely.", decision: true },
      ],
      learned: [
        fact(maren, "role", "Maren is the night clerk of the Harbour Office."),
        arrival,
        fact(office, "rule", "No ledger leaves the counter of the Harbour Office.", maren),
        ledger,
        roster,
      ],
    },
    {
      turn_id: 6,
      place: "The Lantern Steps",
      clock_from: at(3, 18, 10),
      clock_to: at(3, 18, 55),
      present: [orsel, tovin],
      acts: [
        {
          turn_id: 6,
          intent: "I put the ledger back on the counter and walk down to the Lantern Steps.",
          decision: true,
        },
        { turn_id: 7, intent: "I ask the porters who unloaded the Gull's last cargo.", decision: false },
        { turn_id: 8, intent: "I pay Tovin two marks to show me the empty berth.", decision: true },
      ],
      learned: [
        fact(tovin, "role", "Tovin carries cargo for anyone who pays in coin."),
        fact(guild, "cellar", "Tovin says the Gull's crates went down to the Salt Guild's cellar.", tovin),
        fact(steps, "berth_nine", "Berth nine below the steps sits empty, its chain cut."),
        fact(pc, "mood", "Ada mood: uneasy"),
      ],
    },
    {
      turn_id: 9,
      place: "the Guild cellar door",
      clock_from: at(4, 6, 20),
      clock_to: at(4, 6, 50),
      present: [stranger],
      acts: [
        { turn_id: 9, intent: "I wait by the cellar door for the day watch.", decision: false },
        { turn_id: 10, intent: "I follow the man in the grey coat into the alley.", decision: true },
      ],
      learned: [fact(guild, "watch", "The Salt Guild changes the cellar watch at dawn.")],
    },
  ],
  open: [debt, roster, arrival],
  closed: [{ item: { ...ledger, open: false }, closed_in: 6 }],
  known_before: [
    fact(pc, "role", "Ada Quell carries sealed papers for the harbour houses."),
    debt,
    fact(guild, "holds", "The Salt Guild holds the debts of half the harbour."),
    fact(steps, "use", "The night porters wait for work on the Lantern Steps."),
    fact(maren, "known", "Maren has kept the night desk for twenty years."),
  ],
  notes: [
    {
      id: 1,
      body: "Maren knew about the struck-out berth before I asked. Who told her to keep quiet?",
      about: { kind: "scene", turn_id: 1 },
      created_ms: 1_762_700_000_000,
      updated_ms: 1_762_700_000_000,
    },
    {
      id: 2,
      body: "Forty marks by Day 5. Tovin might lend, but he talks.",
      about: null,
      created_ms: 1_762_710_000_000,
      updated_ms: 1_762_712_000_000,
    },
    {
      id: 3,
      body: "The man in the grey coat walks like a Guild clerk. Ask Orsel.",
      about: { kind: "entity", entity_id: 3 },
      created_ms: 1_762_720_000_000,
      updated_ms: 1_762_720_000_000,
    },
  ],
  pins: [
    { kind: "note", note_id: 2 },
    { kind: "act", turn_id: 8 },
    { kind: "scene", turn_id: 6 },
  ],
};
