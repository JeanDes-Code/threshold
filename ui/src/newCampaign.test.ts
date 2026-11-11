import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AdventureSeed, AgeLine, DraftView, EntryRef, GapKind } from "./api";
import {
  CANCEL_NOTE,
  DOCUMENT_MAX_BYTES,
  readDocument,
  addEntry,
  afterFlush,
  ageLabel,
  ageLineOf,
  ageStatus,
  ageTextMatches,
  agesLeft,
  canApprove,
  confirmRow,
  createSaver,
  draftScreen,
  entryOf,
  parseAge,
  parseHour,
  factEditFields,
  cutSections,
  cutSentence,
  failureText,
  fieldId,
  focusTarget,
  gapSentence,
  gapsAt,
  formOfView,
  nameChange,
  parseAt,
  removeAt,
  removeEntry,
  renamed,
  replaceAt,
  progressText,
  slowLine,
  elapsedText,
  saveArgs,
  sectionOf,
  shownName,
  splitList,
  stageLabel,
  typeName,
  withStoredAges,
  type ReviewForm,
} from "./newCampaign";

function form(): ReviewForm {
  const seed: AdventureSeed = {
    premise: "A ferryman carries letters.",
    pc: {
      name: "Teodor Brandt",
      age: 34,
      other_names: [{ name: "Wick", known_to: ["Agathe Morrow"] }],
      concept: "A careful ferryman.",
    },
    entities: [
      { name: "Agathe Morrow", kind: "npc", age: 67, other_names: [], hidden: false },
      { name: "Sefa Lind", kind: "npc", age: 16, other_names: [], hidden: false },
      { name: "", kind: "place", age: null, other_names: [], hidden: false },
    ],
    boundaries: [],
    decided: [],
    proposed: [],
    story_language: "English",
    themes: [],
    gm_agreement: "",
    resolution: "",
    min_age: null,
    start: { place: "Ostley Crossing", present: ["Pim Quast"], situation: "", when: "" },
    open_details: [],
    detail: "excluded",
  };
  return {
    seed,
    knowledge: [
      { subject: "Agathe Morrow", key: "trade", value: "innkeeper", text: "Agathe runs the inn.", open: false },
      { subject: "Marlowe", key: "pending_trip", value: "the coast", text: "", open: true },
    ],
    typed: {},
  };
}

function view(f: ReviewForm, ages: AgeLine[] = []): DraftView {
  return {
    draft_id: "draft-1",
    state: "draft",
    seed: f.seed,
    knowledge: f.knowledge,
    ages,
    cut: [],
    gaps: [],
    last_failure: null,
  };
}

describe("stageLabel", () => {
  it("names each stage in plain words", () => {
    expect(stageLabel("draft_roster")).toBe("Reading the characters");
    expect(stageLabel("draft_seed")).toBe("Reading the story");
    expect(stageLabel("draft_knowledge")).toBe("Reading what your character knows");
    expect(stageLabel("hidden_facts")).toBe("Preparing the world");
    expect(stageLabel("npc_state")).toBe("Preparing characters");
    expect(stageLabel("world_events")).toBe("Preparing events");
    expect(stageLabel("start_state")).toBe("Preparing the start");
    expect(stageLabel("commit")).toBe("Saving the campaign");
  });

  it("falls back to a neutral label for an unknown stage", () => {
    expect(stageLabel("something_new")).toBe("Working");
    expect(stageLabel("")).toBe("Working");
    expect(stageLabel("toString")).toBe("Working");
  });
});

describe("progressText", () => {
  it("gives the stage and the count", () => {
    expect(progressText({ stage: "npc_state", done: 4, total: 9 })).toBe(
      "Preparing characters, 4 of 9",
    );
  });

  it("drops the count when there is none", () => {
    expect(progressText({ stage: "commit", done: 0, total: 0 })).toBe("Saving the campaign");
  });
});

describe("failureText", () => {
  it("names the stage and the check", () => {
    expect(failureText({ stage: "npc_state", check: "completeness" })).toBe(
      "The last build stopped at “Preparing characters”: a completeness check failed.",
    );
    expect(failureText({ stage: "start_state", check: "new_check" })).toBe(
      "The last build stopped at “Preparing the start”: new check.",
    );
  });
});

describe("parseAt", () => {
  it("splits a locator into names and indexes", () => {
    expect(parseAt("premise")).toEqual(["premise"]);
    expect(parseAt("pc.age")).toEqual(["pc", "age"]);
    expect(parseAt("entities[3].other_names[1].known_to[0]")).toEqual([
      "entities",
      3,
      "other_names",
      1,
      "known_to",
      0,
    ]);
    expect(parseAt("knowledge[5]")).toEqual(["knowledge", 5]);
  });
});

describe("focusTarget", () => {
  it("points at the input that holds the field", () => {
    expect(focusTarget("pc.name")).toBe("pc.name");
    expect(focusTarget("entities[3].age")).toBe("entities[3].age");
    expect(focusTarget("entities[0].other_names[1]")).toBe("entities[0].other_names[1].name");
    expect(focusTarget("pc.other_names[0].known_to[2]")).toBe("pc.other_names[0].known_to");
    expect(focusTarget("start.present[2]")).toBe("start.present");
    expect(focusTarget("knowledge[4]")).toBe("knowledge[4]");
    expect(focusTarget("knowledge[4].subject")).toBe("knowledge[4]");
    expect(focusTarget("seed")).toBe("premise");
  });

  it("makes a usable element id", () => {
    expect(fieldId("entities[3].age")).toBe("f-entities-3--age");
    expect(fieldId(focusTarget("pc.name"))).toBe("f-pc-name");
  });
});

describe("gapSentence", () => {
  const sentence = (kind: string, at: string) =>
    gapSentence({ kind: kind as never, at }, form());

  it("each gap sentence says what is wrong and what to do", () => {
    expect(sentence("missing", "premise")).toBe("The premise is empty. Write it in.");
    expect(sentence("age_missing", "entities[1].age")).toBe(
      "The age of Sefa Lind is missing. Enter an age in years.",
    );
    expect(sentence("age_unconfirmed", "entities[1].age")).toBe(
      "The age of Sefa Lind is not confirmed yet. Check it, then press “Confirm age”.",
    );
    expect(sentence("age_unconfirmed", "pc.age")).toBe(
      "Your character's age is not confirmed yet. Check it, then press “Confirm age”.",
    );
    expect(sentence("age_below_min", "entities[1].age")).toBe(
      "The age of Sefa Lind is below the minimum age. Raise the age, or change the minimum age under “How you want to play”.",
    );
    expect(sentence("missing", "entities[2].name")).toBe("The name of entry 3 is empty. Write it in.");
    expect(sentence("unknown_name", "start.present[0]")).toBe(
      "“Pim Quast” in “who is present” is not a character of the draft. Use a character's name, or add the character.",
    );
    expect(sentence("unknown_name", "start.place")).toBe(
      "The start place is not a place of the draft. Use the name of a place below, or add the place.",
    );
    expect(sentence("unknown_name", "pc.other_names[0].known_to[0]")).toBe(
      "“Agathe Morrow” in who knows “Wick” is not a character of the draft. Use a character's name, or delete it.",
    );
    expect(sentence("duplicate_name", "pc.other_names[0]")).toBe(
      "The other name “Wick” of your character is used twice. Give one of them another name.",
    );
    expect(sentence("pc_in_roster", "entities[0].name")).toBe(
      "The name of Agathe Morrow is a name of your character. Delete this entry, or give it another name.",
    );
    expect(sentence("wrong_kind", "entities[2].age")).toBe(
      "The age of entry 3 is set, but only a character has an age. Clear it.",
    );
    expect(sentence("seed_block_too_long", "seed")).toBe(
      "The story setup is too long for the GM to read. Shorten the premise, the GM agreement or the content limits.",
    );
  });

  it("speaks of a fact by its sentence, never by its internal fields", () => {
    expect(sentence("slot_conflict", "knowledge[0]")).toBe(
      "The fact “Agathe runs the inn.” says something about the same thing as another fact. Delete one of them, or merge them.",
    );
    expect(sentence("missing", "knowledge[0].value")).toBe(
      "“Value” in the fact “Agathe runs the inn.” is empty. Write it in.",
    );
    expect(sentence("unknown_name", "knowledge[1].subject")).toBe(
      "Fact 2 is about “Marlowe”, who is not in the draft. Press “edit” and change “About”, or add “Marlowe” as a character or a place.",
    );
    expect(sentence("wrong_kind", "knowledge[1].key")).toBe(
      "Fact 2 has “pending” in its detail. Press “edit”, remove “pending” from “Detail”, and tick “Not done yet” instead.",
    );
    for (const at of ["knowledge[0]", "knowledge[0].subject", "knowledge[0].key", "knowledge[1].key"]) {
      for (const kind of ["missing", "unknown_name", "wrong_kind", "slot_conflict"]) {
        expect(sentence(kind, at)).not.toMatch(/\b(subject|key)\b/);
      }
    }
  });

  it("gives every kind two sentences: the problem, then the fix", () => {
    const kinds: GapKind[] = [
      "missing",
      "age_missing",
      "age_unconfirmed",
      "age_below_min",
      "unknown_name",
      "duplicate_name",
      "pc_in_roster",
      "wrong_kind",
      "seed_block_too_long",
      "slot_conflict",
    ];
    for (const kind of [...kinds, "new_kind"]) {
      const text = sentence(kind, "entities[0].age");
      expect(text.split(/(?<=\.) /).length, text).toBeGreaterThanOrEqual(2);
    }
    expect(sentence("new_kind", "premise")).toBe("The premise needs a fix. Check it.");
  });

  it("keeps an unknown locator as it is", () => {
    expect(sentence("missing", "decided[0].subject")).toBe("Decided fact 1 is empty. Write it in.");
    expect(sentence("missing", "weird.path")).toBe("weird.path is empty. Write it in.");
  });
});

describe("review sections", () => {
  it("puts each gap in the part of the review that holds its field", () => {
    const f = form();
    expect(sectionOf("premise", f)).toBe("story");
    expect(sectionOf("start.present[0]", f)).toBe("story");
    expect(sectionOf("pc.other_names[0].known_to[0]", f)).toBe("you");
    expect(sectionOf("entities[1].age", f)).toBe("people");
    expect(sectionOf("entities[2].name", f)).toBe("places");
    expect(sectionOf("knowledge[0].subject", f)).toBe("knows");
    expect(sectionOf("knowledge[1]", f)).toBe("open");
    expect(sectionOf("min_age", f)).toBe("play");
  });

  it("finds the gaps of one field and of the fields inside it", () => {
    const gaps = [
      { kind: "missing" as const, at: "entities[1].name" },
      { kind: "missing" as const, at: "entities[10].name" },
      { kind: "slot_conflict" as const, at: "knowledge[1]" },
      { kind: "missing" as const, at: "knowledge[1].value" },
    ];
    expect(gapsAt(gaps, "entities[1]").map((g) => g.at)).toEqual(["entities[1].name"]);
    expect(gapsAt(gaps, "knowledge[1]")).toHaveLength(2);
    expect(gapsAt(gaps, "entities[1].name")).toHaveLength(1);
  });
});

describe("ages", () => {
  const line = (
    name: string,
    source: AgeLine["source"],
    confirmed = false,
    index: number | null = null,
    kind: AgeLine["kind"] = index === null ? "pc" : "npc",
  ): AgeLine => ({ name, index, kind, source, confirmed });

  it("an age label says edited by you only for an edited age", () => {
    expect(ageLabel(line("Teodor Brandt", "from_text"))).toBe("from the text");
    expect(ageLabel(line("Sefa Lind", "proposed"))).toBe("proposed by the model");
    expect(ageLabel(line("Agathe Morrow", "edited"))).toBe("edited by you");
    // A confirmation does not change where the age comes from.
    expect(ageLabel(line("Agathe Morrow", "edited", true))).toBe("edited by you");
    expect(ageLabel(line("Teodor Brandt", "from_text", true))).toBe("from the text");
    // A row the core has no line for yet is the Player's own.
    expect(ageLabel(undefined)).toBe("edited by you");
  });

  it("reads each line by its row, saved name and kind, and counts the ages left", () => {
    const ages = [
      line("Teodor Brandt", "from_text", true),
      line("Agathe Morrow", "edited", false, 0),
      line("Sefa Lind", "proposed", false, 1),
    ];
    const f = form();
    expect(ageLineOf(ages, f, 0)?.source).toBe("edited");
    expect(ageLineOf(ages, f, "pc")?.confirmed).toBe(true);
    // No name matching of its own: the UI reads the core's lines as they are.
    const lower = { ...f, seed: { ...f.seed, entities: replaceAt(f.seed.entities, 0, { name: "agathe morrow" }) } };
    expect(ageLineOf(ages, lower, 0)).toBeUndefined();
    // A line of another row, or of another kind, is not this row's.
    const moved = [line("Agathe Morrow", "edited", true, 1)];
    expect(ageLineOf(moved, f, 0)).toBeUndefined();
    const placed = [line("Agathe Morrow", "edited", true, 0, "place")];
    expect(ageLineOf(placed, f, 0)).toBeUndefined();
    expect(agesLeft(ages)).toBe(2);
  });

  it("reads decimal digits only", () => {
    expect(parseAge("16")).toBe(16);
    expect(parseAge(" 40 ")).toBe(40);
    expect(parseAge("")).toBeNull();
    expect(parseAge("   ")).toBeNull();
    for (const text of ["0x10", "1e1", "16.0", "16.5", "-3", "+16", "1_0", "١٦", "16a", "65536"]) {
      expect(parseAge(text), text).toBeUndefined();
    }
    expect(parseAge("65535")).toBe(65535);
  });

  it("edits a fact's sentence only, outside debug mode", () => {
    expect(factEditFields(false)).toEqual([]);
    expect(factEditFields(true)).toEqual(["subject", "key", "value"]);
  });

  it("reads a start hour 0 to 23, empty as the default", () => {
    expect(parseHour("19")).toBe(19);
    expect(parseHour(" 0 ")).toBe(0);
    expect(parseHour("23")).toBe(23);
    expect(parseHour("")).toBeNull();
    for (const text of ["24", "-1", "7.5", "7h", "abc", "100"]) {
      expect(parseHour(text), text).toBeUndefined();
    }
  });

  it("shows confirmed only while the field shows the confirmed, saved age", () => {
    const confirmed = line("Agathe Morrow", "edited", true, 0);
    const settled = { text: null, value: 67, line: confirmed, pending: false, saveFailed: false, busy: false };
    expect(ageStatus(settled)).toEqual({ confirmed: true, canConfirm: false });
    // The field changes: the label drops at once, before any save.
    expect(ageStatus({ ...settled, text: "6" })).toEqual({ confirmed: false, canConfirm: false });
    // Text the form never took ("0x43" is 67 in hex) is not the age.
    expect(ageStatus({ ...settled, text: "0x43" })).toEqual({ confirmed: false, canConfirm: false });
    expect(ageStatus({ ...settled, text: "067" }).confirmed).toBe(false);
    // The same text as the age is fine.
    expect(ageStatus({ ...settled, text: "67" }).confirmed).toBe(true);
    // A waiting or failed save: neither the label nor the button.
    expect(ageStatus({ ...settled, pending: true })).toEqual({ confirmed: false, canConfirm: false });
    expect(ageStatus({ ...settled, saveFailed: true })).toEqual({ confirmed: false, canConfirm: false });
  });

  it("lets the Player confirm only an age the field shows and the store holds", () => {
    const open = line("Sefa Lind", "proposed", false, 1);
    const settled = { text: null, value: 16, line: open, pending: false, saveFailed: false, busy: false };
    expect(ageStatus(settled)).toEqual({ confirmed: false, canConfirm: true });
    expect(ageStatus({ ...settled, text: "0x10" }).canConfirm).toBe(false);
    expect(ageStatus({ ...settled, text: "17" }).canConfirm).toBe(false);
    expect(ageStatus({ ...settled, pending: true }).canConfirm).toBe(false);
    expect(ageStatus({ ...settled, saveFailed: true }).canConfirm).toBe(false);
    expect(ageStatus({ ...settled, busy: true }).canConfirm).toBe(false);
    expect(ageStatus({ ...settled, value: null }).canConfirm).toBe(false);
    // No line yet (a row the core has not seen): the button works once saved.
    expect(ageStatus({ ...settled, line: undefined }).canConfirm).toBe(true);
    expect(ageTextMatches(null, null)).toBe(false);
  });
});

describe("review actions", () => {
  it("confirm flushes the saver, then sends the row's saved name and kind", async () => {
    const calls: unknown[] = [];
    const flush = () => {
      calls.push("flush");
      return Promise.resolve();
    };
    const confirm = (entry: EntryRef) => {
      calls.push(entry);
      return Promise.resolve("done");
    };
    // A typed name is not the entry: the core gets the saved one.
    const f = typeName(form(), 1, "Sefa Marsh");
    await expect(confirmRow(flush, confirm, f, 1)).resolves.toBe("done");
    await confirmRow(flush, confirm, f, "pc");
    expect(calls).toEqual([
      "flush",
      { index: 1, name: "Sefa Lind", kind: "npc" },
      "flush",
      { index: null, name: "Teodor Brandt", kind: "pc" },
    ]);
  });

  it("a failed flush sends nothing", async () => {
    const confirm = vi.fn(() => Promise.resolve("done"));
    const flush = () => Promise.reject(new Error("disk full"));
    await expect(confirmRow(flush, confirm, form(), 0)).rejects.toThrow("disk full");
    await expect(afterFlush(flush, confirm)).rejects.toThrow("disk full");
    expect(confirm).not.toHaveBeenCalled();
  });

  it("the entry of a row is its index, saved name and kind", () => {
    const f = form();
    expect(entryOf(f, "pc")).toEqual({ index: null, name: "Teodor Brandt", kind: "pc" });
    expect(entryOf(f, 2)).toEqual({ index: 2, name: "", kind: "place" });
  });

  it("approve waits for every save, no gap, and every age field showing its age", () => {
    const ready = {
      pending: false,
      busy: false,
      approving: false,
      saveFailed: false,
      error: null,
      gaps: [],
      state: "draft" as const,
      agesShown: true,
    };
    expect(canApprove(ready)).toBe(true);
    expect(canApprove({ ...ready, pending: true })).toBe(false);
    expect(canApprove({ ...ready, busy: true })).toBe(false);
    expect(canApprove({ ...ready, approving: true })).toBe(false);
    expect(canApprove({ ...ready, saveFailed: true })).toBe(false);
    expect(canApprove({ ...ready, error: "Could not save" })).toBe(false);
    expect(canApprove({ ...ready, gaps: [{ kind: "age_unconfirmed", at: "pc.age" }] })).toBe(false);
    expect(canApprove({ ...ready, agesShown: false })).toBe(false);
    expect(canApprove({ ...ready, state: "built" })).toBe(false);
  });

  it("an approved draft with a gap opens in the review, not the build", () => {
    const approved = { ...view(form()), state: "approved" as const };
    expect(draftScreen(approved)).toBe("build");
    expect(draftScreen({ ...approved, gaps: [{ kind: "age_unconfirmed", at: "pc.age" }] })).toBe("review");
    expect(draftScreen({ ...approved, state: "draft" })).toBe("review");
    expect(draftScreen({ ...approved, state: "built" })).toBe("review");
  });
});

describe("names", () => {
  it("a typed name stays out of the saved seed until the rename returns", () => {
    let f = form();
    for (const text of ["A", "Ag", "Aggie", "Aggie Morrow"]) {
      f = typeName(f, 0, text);
      // The field shows what the Player types; the store gets the old name.
      expect(shownName(f, 0)).toBe(text);
      expect(saveArgs(f).seed.entities[0]?.name).toBe("Agathe Morrow");
      expect(saveArgs(f)).not.toHaveProperty("typed");
    }
    expect(nameChange(f, 0)).toEqual({
      kind: "rename",
      entry: { index: 0, name: "Agathe Morrow", kind: "npc" },
      to: "Aggie Morrow",
    });

    // The core renames every reference; the form takes the store's seed.
    const seed = {
      ...f.seed,
      entities: replaceAt(f.seed.entities, 0, { name: "Aggie Morrow" }),
      pc: { ...f.seed.pc, other_names: [{ name: "Wick", known_to: ["Aggie Morrow"] }] },
    };
    const knowledge = replaceAt(f.knowledge, 0, { subject: "Aggie Morrow" });
    f = renamed(f, view({ ...f, seed, knowledge }), 0);
    expect(f.typed).toEqual({});
    expect(saveArgs(f).seed.entities[0]?.name).toBe("Aggie Morrow");
    expect(saveArgs(f).seed.pc.other_names[0]?.known_to).toEqual(["Aggie Morrow"]);
    expect(saveArgs(f).knowledge[0]?.subject).toBe("Aggie Morrow");
    expect(nameChange(f, 0)).toEqual({ kind: "none" });
  });

  it("commits nothing for an unchanged name, and saves a name that had none", () => {
    let f = typeName(form(), "pc", "  Teodor Brandt ");
    expect(nameChange(f, "pc")).toEqual({ kind: "none" });
    f = typeName(form(), 2, "Ostley Crossing");
    expect(nameChange(f, 2)).toEqual({ kind: "save", to: "Ostley Crossing" });
    expect(nameChange(form(), 1)).toEqual({ kind: "none" });
  });

  it("adding and deleting a row keep the other rows", () => {
    let f = typeName(form(), "pc", "Teo");
    f = addEntry(f, { name: "Nell Quay", kind: "npc", age: 44, other_names: [], hidden: false });
    expect(f.seed.entities.map((e) => e.name)).toEqual(["Agathe Morrow", "Sefa Lind", "", "Nell Quay"]);
    f = typeName(f, 1, "Sefa");
    f = removeEntry(f, 0);
    expect(f.seed.entities.map((e) => e.name)).toEqual(["Sefa Lind", "", "Nell Quay"]);
    // A typed roster name loses its row; the PC's stays.
    expect(f.typed).toEqual({ pc: "Teo" });
  });

  it("a new view starts with nothing typed", () => {
    expect(formOfView(view(form())).typed).toEqual({});
  });
});

describe("cut lists", () => {
  it("a cut list says it was cut", () => {
    const f = form();
    const say = (at: string, cap: number) => cutSentence({ at, cap }, f);
    expect(say("entities", 30)).toBe(
      "Your document names more than 30 people, or more than 30 places and groups. The draft kept the first 30 of each: add any missing one with “Add a character” or “Add a place”.",
    );
    expect(say("knowledge", 40)).toBe(
      "A part of your document holds more than 40 facts. The draft kept the first 40 of that part, so some facts may be missing.",
    );
    expect(say("entities[0].other_names", 4)).toBe(
      "Agathe Morrow has more than 4 other names in your document. The draft kept the first 4.",
    );
    expect(say("pc.other_names", 4)).toBe(
      "Your character has more than 4 other names in your document. The draft kept the first 4.",
    );
    expect(say("themes", 8)).toBe("Your document gives more than 8 themes. The draft kept the first 8.");
    expect(say("somewhere.new", 5)).toBe("A list of the draft was cut at 5 items.");
    for (const at of ["entities", "knowledge", "themes", "start.present", "open_details"]) {
      expect(say(at, 3)).toMatch(/kept the first 3|cut at 3/);
    }
  });

  it("shows each cut in the part of the review it concerns", () => {
    const f = form();
    expect(cutSections({ at: "entities", cap: 30 }, f)).toEqual(["people", "places"]);
    expect(cutSections({ at: "knowledge", cap: 40 }, f)).toEqual(["knows", "open"]);
    expect(cutSections({ at: "entities[0].other_names", cap: 4 }, f)).toEqual(["people"]);
    expect(cutSections({ at: "themes", cap: 8 }, f)).toEqual(["play"]);
  });
});

describe("list edits", () => {
  it("patch or drop one item and leave the list as it was", () => {
    const list = [{ a: 1 }, { a: 2 }];
    expect(replaceAt(list, 1, { a: 5 })).toEqual([{ a: 1 }, { a: 5 }]);
    expect(removeAt(list, 0)).toEqual([{ a: 2 }]);
    expect(list).toEqual([{ a: 1 }, { a: 2 }]);
  });
});

describe("splitList", () => {
  it("splits, trims and drops empty items", () => {
    expect(splitList("Agathe Morrow, Sefa Lind,, ", ",")).toEqual(["Agathe Morrow", "Sefa Lind"]);
    expect(splitList("one\n\ntwo, three\n", "\n")).toEqual(["one", "two, three"]);
  });
});

describe("createSaver", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function setup() {
    const sent: string[] = [];
    const finish: (() => void)[] = [];
    const results: string[] = [];
    const pending: boolean[] = [];
    const saver = createSaver<string, string>({
      delayMs: 600,
      save: (form) => {
        sent.push(form);
        return new Promise((resolve) => finish.push(() => resolve(`saved ${form}`)));
      },
      onResult: (r) => results.push(r),
      onError: () => undefined,
      onPending: (on) => pending.push(on),
    });
    return { saver, sent, finish, results, pending };
  }

  it("waits for typing to pause", () => {
    const { saver, sent, pending } = setup();
    saver.edit("a");
    vi.advanceTimersByTime(300);
    saver.edit("ab");
    vi.advanceTimersByTime(599);
    expect(sent).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(sent).toEqual(["ab"]);
    expect(pending).toEqual([true]);
  });

  it("sends at once when asked", () => {
    const { saver, sent } = setup();
    saver.edit("a", true);
    expect(sent).toEqual(["a"]);
  });

  it("runs one save at a time and sends only the latest form", async () => {
    const { saver, sent, finish, results, pending } = setup();
    saver.edit("a", true);
    saver.edit("ab", true);
    saver.edit("abc", true);
    expect(sent).toEqual(["a"]);
    finish[0]?.();
    await vi.runAllTimersAsync();
    expect(sent).toEqual(["a", "abc"]);
    expect(results).toEqual([]);
    finish[1]?.();
    await vi.runAllTimersAsync();
    expect(results).toEqual(["saved abc"]);
    expect(pending).toEqual([true, false]);
  });

  it("stays pending while a newer edit waits for its timer", async () => {
    const { saver, finish, results, pending } = setup();
    saver.edit("a", true);
    saver.edit("ab");
    finish[0]?.();
    await Promise.resolve();
    await Promise.resolve();
    expect(results).toEqual([]);
    expect(pending).toEqual([true]);
  });

  it("start() after stop() reports again", async () => {
    const { saver, finish, results } = setup();
    saver.stop();
    saver.start();
    saver.edit("a", true);
    finish[0]?.();
    await vi.runAllTimersAsync();
    expect(results).toEqual(["saved a"]);
  });

  it("flush() sends the last edit at once and waits for it", async () => {
    const { saver, sent, finish } = setup();
    saver.edit("a");
    let done = false;
    const flushed = saver.flush().then(() => (done = true));
    expect(sent).toEqual(["a"]);
    await Promise.resolve();
    expect(done).toBe(false);
    finish[0]?.();
    await flushed;
    expect(done).toBe(true);
  });

  it("flush() fails when the last save fails", async () => {
    const saver = createSaver<string, string>({
      delayMs: 600,
      save: () => Promise.reject(new Error("disk full")),
      onResult: () => undefined,
      onError: () => undefined,
      onPending: () => undefined,
    });
    saver.edit("a");
    await expect(saver.flush()).rejects.toThrow("disk full");
  });

  /** A saver whose saves succeed or fail in the order of `outcomes`. */
  function scripted(outcomes: ("ok" | "fail")[]) {
    const sent: string[] = [];
    const saver = createSaver<string, string>({
      delayMs: 600,
      save: (form) => {
        sent.push(form);
        return outcomes.shift() === "fail"
          ? Promise.reject(new Error(`could not save ${form}`))
          : Promise.resolve(form);
      },
      onResult: () => undefined,
      onError: () => undefined,
      onPending: () => undefined,
    });
    return { saver, sent };
  }

  it("flush() retries a form whose save failed before, and resolves when it succeeds", async () => {
    const { saver, sent } = scripted(["fail", "ok"]);
    saver.edit("a", true);
    await vi.runAllTimersAsync();
    await expect(saver.flush()).resolves.toBeUndefined();
    expect(sent).toEqual(["a", "a"]);
  });

  it("flush() rejects when the retry of a failed form fails again", async () => {
    const { saver, sent } = scripted(["fail", "fail"]);
    saver.edit("a", true);
    await vi.runAllTimersAsync();
    await expect(saver.flush()).rejects.toThrow("could not save a");
    expect(sent).toEqual(["a", "a"]);
  });

  it("after a rejected flush(), the next flush() does not resolve silently", async () => {
    const { saver, sent } = scripted(["fail", "fail", "fail", "ok"]);
    saver.edit("a", true);
    await vi.runAllTimersAsync();
    await expect(saver.flush()).rejects.toThrow();
    await expect(saver.flush()).rejects.toThrow("could not save a");
    await expect(saver.flush()).resolves.toBeUndefined();
    expect(sent).toEqual(["a", "a", "a", "a"]);
  });

  it("a later save that succeeds clears the failed form", async () => {
    const { saver, sent } = scripted(["fail", "ok"]);
    saver.edit("a", true);
    await vi.runAllTimersAsync();
    saver.edit("ab", true);
    await vi.runAllTimersAsync();
    await expect(saver.flush()).resolves.toBeUndefined();
    expect(sent).toEqual(["a", "ab"]);
  });

  it("flush() with nothing to save resolves", async () => {
    const { saver } = setup();
    await expect(saver.flush()).resolves.toBeUndefined();
  });

  it("stop() still saves the last edit, and reports nothing", async () => {
    const { saver, sent, finish, results } = setup();
    saver.edit("a");
    saver.stop();
    expect(sent).toEqual(["a"]);
    finish[0]?.();
    await vi.runAllTimersAsync();
    expect(results).toEqual([]);
  });
});

describe("readDocument", () => {
  const file = (size: number) => {
    const text = vi.fn(() => Promise.resolve("the text"));
    return { file: { size, text }, text };
  };

  it("refuses a file above 1 MB before reading it", async () => {
    expect(DOCUMENT_MAX_BYTES).toBe(1024 * 1024);
    const big = file(DOCUMENT_MAX_BYTES + 1);
    expect(await readDocument(big.file)).toEqual({
      ok: false,
      message: "This file is larger than 1 MB. Use a smaller file, or paste the part you need.",
    });
    expect(big.text).not.toHaveBeenCalled();
  });

  it("reads a file at the limit", async () => {
    const ok = file(DOCUMENT_MAX_BYTES);
    expect(await readDocument(ok.file)).toEqual({ ok: true, text: "the text" });
    expect(ok.text).toHaveBeenCalledOnce();
  });
});

describe("CANCEL_NOTE", () => {
  it("says when a cancel takes effect", () => {
    // Changed after review: a cancel stops the running model call (C14).
    expect(CANCEL_NOTE).toBe("Cancel stops the current model call.");
  });
});

describe("a slow model (C13)", () => {
  const p = { stage: "hidden_facts", done: 0, total: 8, attempt: 1, attempts: 3, retry_after_timeout: false };
  it("says nothing on a first attempt", () => {
    expect(slowLine(p)).toBeNull();
  });
  it("says plainly that the model is slow after a timeout", () => {
    expect(slowLine({ ...p, attempt: 2, retry_after_timeout: true })).toBe(
      "The model is slow or not answering. Attempt 2 of 3.",
    );
  });
  it("shows the elapsed time of the stage", () => {
    expect(elapsedText(0)).toBe("0 s");
    expect(elapsedText(45_900)).toBe("45 s");
    expect(elapsedText(9 * 60_000 + 5_000)).toBe("9 min 05 s");
  });
});

describe("cancel and timeout failures (C15)", () => {
  it("says the Player cancelled, and a timeout says what to check", () => {
    expect(failureText({ stage: "hidden_facts", check: "cancelled" })).toBe(
      "The last build stopped at “Preparing the world”: you cancelled it.",
    );
    expect(failureText({ stage: "hidden_facts", check: "model_timeout" })).toBe(
      "The last build stopped at “Preparing the world”: the model did not answer in time. Check that no other program is using the machine's memory, then retry.",
    );
  });
});

describe("age facts after a store answer", () => {
  it("the form takes the age sentence the store rewrote, and nothing else", () => {
    // C11 (review): QA set Agathe's age to 68 and pressed Confirm age; the
    // screen still read "Agathe Morrow is 67 years old."
    const held = form();
    held.knowledge = [
      ...held.knowledge,
      { subject: "Agathe Morrow", key: "age", value: "67", text: "Agathe Morrow is 67 years old.", open: false },
    ];
    held.seed.entities = replaceAt(held.seed.entities, 0, { age: 68 });
    const stored = view({
      ...held,
      knowledge: replaceAt(held.knowledge, 2, { value: "68", text: "Agathe Morrow is 68 years old." }),
    });
    const after = withStoredAges(held, stored);
    expect(after.knowledge[2]).toMatchObject({ value: "68", text: "Agathe Morrow is 68 years old." });
    expect(after.knowledge.slice(0, 2)).toEqual(held.knowledge.slice(0, 2));
    // A row that changed under the Player's hands keeps the Player's text.
    const moved = view({ ...held, knowledge: held.knowledge.slice(1) });
    expect(withStoredAges(held, moved)).toBe(held);
  });
});
