import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AdventureSeed, DraftView } from "./api";
import {
  ageSource,
  agesStated,
  agesToConfirm,
  confirmRow,
  createSaver,
  failureText,
  fieldId,
  focusTarget,
  gapSentence,
  flagsFromNames,
  formOfView,
  isUnconfirmed,
  parseAt,
  removeAt,
  removeEntityRow,
  replaceAt,
  progressText,
  splitList,
  stageLabel,
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
  };
  return {
    seed,
    knowledge: [
      { subject: "Agathe Morrow", key: "trade", value: "innkeeper", text: "Agathe runs the inn.", open: false },
    ],
    unconfirmed: { pc: false, entities: [false, true, false] },
    stated: { pc: true, entities: [true, false, false] },
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
    expect(focusTarget("knowledge[4]")).toBe("knowledge[4].text");
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

  it("names the field in plain words", () => {
    expect(sentence("missing", "premise")).toBe("The premise is empty.");
    expect(sentence("age_unconfirmed", "entities[1].age")).toBe(
      "The age of Sefa Lind is not confirmed: check it, then confirm it.",
    );
    expect(sentence("age_unconfirmed", "pc.age")).toBe(
      "Your character's age is not confirmed: check it, then confirm it.",
    );
    expect(sentence("missing", "entities[2].name")).toBe("The name of entry 3 is empty.");
    expect(sentence("unknown_name", "start.present[0]")).toBe(
      "“Pim Quast” in “who is present” does not match a name of the right kind.",
    );
    expect(sentence("unknown_name", "pc.other_names[0].known_to[0]")).toBe(
      "“Agathe Morrow” in who knows “Wick” does not match a name of the right kind.",
    );
    expect(sentence("slot_conflict", "knowledge[0]")).toBe(
      "Known fact 1 repeats another fact on the same thing.",
    );
    expect(sentence("missing", "knowledge[0].key")).toBe("The key of known fact 1 is empty.");
    expect(sentence("seed_block_too_long", "seed")).toBe("The seed is too long: shorten some text.");
    expect(sentence("age_below_min", "entities[1].age")).toBe(
      "The age of Sefa Lind is below the minimum age.",
    );
  });

  it("keeps an unknown locator as it is", () => {
    expect(sentence("missing", "decided[0].subject")).toBe("Decided fact 1 (subject) is empty.");
    expect(sentence("missing", "weird.path")).toBe("weird.path is empty.");
  });

  it("has a sentence for an unknown kind", () => {
    expect(sentence("new_kind", "premise")).toBe("The premise needs a fix.");
  });
});

describe("ages to confirm", () => {
  /** A form whose characters have these names; `flags` marks the unconfirmed ones. */
  function roster(names: string[], flags: boolean[]): ReviewForm {
    const base = form();
    return {
      ...base,
      seed: {
        ...base.seed,
        entities: names.map((name) => ({
          name,
          kind: "npc" as const,
          age: 40,
          other_names: [],
          hidden: false,
        })),
      },
      unconfirmed: { pc: false, entities: flags },
      stated: { pc: false, entities: names.map(() => false) },
    };
  }
  const rename = (f: ReviewForm, i: number, name: string): ReviewForm => ({
    ...f,
    seed: { ...f.seed, entities: replaceAt(f.seed.entities, i, { name }) },
  });

  it("reads the drafted names as the core matches them", () => {
    const f = roster(["Sefa Lind", "Chloé", "The Copper Kettle", "Agathe Morrow"], []);
    expect(flagsFromNames(f.seed, ["sefa lind.", "CHLOE", "copper kettle"])).toEqual({
      pc: false,
      entities: [true, true, true, false],
    });
    expect(flagsFromNames(f.seed, ["TEODOR BRANDT"]).pc).toBe(true);
  });

  it("typing another row's name, key by key, never confirms that row", () => {
    let f = roster(["Agathe", "Gerrit"], [false, true]);
    for (const name of ["G", "Ge", "Gerrit", "Gerrit ", "Gerrit V", "Gerrit Vane"]) {
      f = rename(f, 0, name);
      expect(isUnconfirmed(f, 1)).toBe(true);
      expect(agesToConfirm(f)).toEqual(["Gerrit"]);
    }
    expect(isUnconfirmed(f, 0)).toBe(false);
  });

  it("two rows with one name stay unconfirmed when one is renamed", () => {
    let f = roster(["Sefa", "Sefa"], [true, true]);
    f = rename(f, 0, "Sefa Lind");
    expect([isUnconfirmed(f, 0), isUnconfirmed(f, 1)]).toEqual([true, true]);
    expect(agesToConfirm(f)).toEqual(["Sefa Lind", "Sefa"]);
  });

  it("deleting a row drops only that row's entry", () => {
    let f = roster(["Ann", "Bo", "Cy"], [true, false, true]);
    f = removeEntityRow(f, 1);
    expect(f.seed.entities.map((e) => e.name)).toEqual(["Ann", "Cy"]);
    expect(f.unconfirmed.entities).toEqual([true, true]);
    f = removeEntityRow(f, 0);
    expect(agesToConfirm(f)).toEqual(["Cy"]);
    expect(isUnconfirmed(f, 0)).toBe(true);
  });

  it("a confirmed row renamed to an unconfirmed row's name leaves that row unconfirmed", () => {
    let f = roster(["Agathe", "Gerrit"], [false, true]);
    f = rename(f, 0, "Gerrit");
    expect(isUnconfirmed(f, 1)).toBe(true);
    // The wire format names ages, so the store sees both rows as unconfirmed.
    expect(isUnconfirmed(f, 0)).toBe(true);
    expect(agesToConfirm(f)).toEqual(["Gerrit"]);
    // Confirming one of two rows with one name confirms neither.
    f = confirmRow(f, 0);
    expect([isUnconfirmed(f, 0), isUnconfirmed(f, 1)]).toEqual([true, true]);
    f = confirmRow(f, 1);
    expect(agesToConfirm(f)).toEqual([]);
  });

  it("a new draft asks for every age, the PC's too, whatever the model says", () => {
    const base = form();
    const view: DraftView = {
      draft_id: "draft-1",
      state: "draft",
      seed: base.seed,
      knowledge: base.knowledge,
      ages_to_confirm: ["Teodor Brandt", "Agathe Morrow", "Sefa Lind"],
      ages_stated: ["Teodor Brandt", "Agathe Morrow", "Sefa Lind"],
      gaps: [],
      last_failure: null,
    };
    let f = formOfView(view);
    expect(f.unconfirmed).toEqual({ pc: true, entities: [true, true, false] });
    expect(isUnconfirmed(f, "pc")).toBe(true);
    expect(ageSource(f, "pc")).toBe("from the text");
    // The PC row confirms on its own; the other rows still wait.
    f = confirmRow(f, "pc");
    expect(isUnconfirmed(f, "pc")).toBe(false);
    expect(agesToConfirm(f)).toEqual(["Agathe Morrow", "Sefa Lind"]);
    // A stated claim never confirms a row.
    expect(isUnconfirmed(f, 0)).toBe(true);
  });

  it("labels each age with the model's claim, kept per row", () => {
    let f = form();
    expect(ageSource(f, "pc")).toBe("from the text");
    expect(ageSource(f, 0)).toBe("from the text");
    expect(ageSource(f, 1)).toBe("proposed by the model");
    // A rename keeps the label on its row and saves it under the new name.
    f = { ...f, seed: { ...f.seed, entities: replaceAt(f.seed.entities, 0, { name: "Aggie" }) } };
    expect(ageSource(f, 0)).toBe("from the text");
    expect(agesStated(f)).toEqual(["Teodor Brandt", "Aggie"]);
    // Deleting a row drops its label and no other.
    f = removeEntityRow(f, 0);
    expect(f.stated.entities).toEqual([false, false]);
    expect(agesStated(f)).toEqual(["Teodor Brandt"]);
  });

  it("confirming a row clears its own entry only", () => {
    let f = roster(["Ann", "Bo"], [true, true]);
    f = { ...f, unconfirmed: { ...f.unconfirmed, pc: true } };
    f = confirmRow(f, "pc");
    expect(isUnconfirmed(f, "pc")).toBe(false);
    expect(agesToConfirm(f)).toEqual(["Ann", "Bo"]);
    f = confirmRow(f, 0);
    expect(agesToConfirm(f)).toEqual(["Bo"]);
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
