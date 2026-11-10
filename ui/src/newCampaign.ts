// Pure helpers of the New Campaign screen: labels, gap and cut sentences,
// locators, the review's sections, typed names and the draft saver.

import type {
  AdventureSeed,
  AgeLine,
  BuildFailure,
  BuildProgress,
  BuildStage,
  Cut,
  DraftState,
  DraftView,
  EntityKind,
  EntryRef,
  Gap,
  GapKind,
  KnownFact,
  SeedEntity,
} from "./api";

/**
 * What the review edits. `save_draft` takes `seed` and `knowledge` only.
 * `typed` holds a name the Player is typing, by row: it stays out of the
 * seed until the core's rename returns, so a half-typed name never reaches
 * the store and never makes a new entry.
 */
export interface ReviewForm {
  seed: AdventureSeed;
  knowledge: KnownFact[];
  typed: Partial<Record<string, string>>;
}

/** A roster row: the PC, or an index in `seed.entities`. */
export type Row = "pc" | number;

const STAGES: Record<BuildStage, string> = {
  draft_roster: "Reading the characters",
  draft_seed: "Reading the story",
  draft_knowledge: "Reading what your character knows",
  hidden_facts: "Preparing the world",
  npc_state: "Preparing characters",
  world_events: "Preparing events",
  start_state: "Preparing the start",
  commit: "Saving the campaign",
};

/** The largest document file the screen reads, in bytes: 1 MB. The core takes 40,000 characters. */
export const DOCUMENT_MAX_BYTES = 1024 * 1024;

/** Shown next to "Cancel": a cancel waits for the model call that runs. */
export const CANCEL_NOTE = "Cancel takes effect after the current step.";

export type DocumentRead = { ok: true; text: string } | { ok: false; message: string };

/** Reads a chosen document, unless it is above `DOCUMENT_MAX_BYTES`: then it reads nothing. */
export async function readDocument(file: {
  size: number;
  text: () => Promise<string>;
}): Promise<DocumentRead> {
  if (file.size > DOCUMENT_MAX_BYTES) {
    return {
      ok: false,
      message: "This file is larger than 1 MB. Use a smaller file, or paste the part you need.",
    };
  }
  return { ok: true, text: await file.text() };
}

/** The Player's label of a stage; "Working" for a stage this UI does not know. */
export function stageLabel(stage: string): string {
  return Object.hasOwn(STAGES, stage) ? STAGES[stage as BuildStage] : "Working";
}

/** "Preparing characters, 4 of 9". */
export function progressText(p: BuildProgress): string {
  const label = stageLabel(p.stage);
  return p.total > 0 ? `${label}, ${p.done} of ${p.total}` : label;
}

const CHECKS: Record<string, string> = {
  model: "the model call failed",
  schema: "the model answer did not fit its form",
  references: "a reference check failed",
  one_pc: "the one-PC check failed",
  kinds: "a kind check failed",
  ages: "an age check failed",
  completeness: "a completeness check failed",
  agency: "a PC agency check failed",
  settled_facts: "a settled facts check failed",
  open_items: "an open items check failed",
  world_autonomy: "a world autonomy check failed",
  secrecy: "a secrecy check failed",
  cancelled: "it was cancelled",
  not_approved: "the draft is not approved",
};

/** The stored failure of a draft's last build, in plain words. */
export function failureText(f: BuildFailure): string {
  const check = Object.hasOwn(CHECKS, f.check) ? CHECKS[f.check] : f.check.replace(/_/g, " ");
  return `The last build stopped at “${stageLabel(f.stage)}”: ${check}.`;
}

/** "entities[3].age" → ["entities", 3, "age"]. */
export function parseAt(at: string): (string | number)[] {
  const parts: (string | number)[] = [];
  for (const m of at.matchAll(/([^.[\]]+)|\[(\d+)\]/g)) {
    parts.push(m[2] !== undefined ? Number(m[2]) : (m[1] ?? ""));
  }
  return parts;
}

/** The field path of the input that holds the field at `at`. */
export function focusTarget(at: string): string {
  if (at === "seed") return "premise";
  let target = at.replace(/\.known_to\[\d+\]$/, ".known_to");
  target = target.replace(/^start\.present\[\d+\]$/, "start.present");
  if (/other_names\[\d+\]$/.test(target)) target += ".name";
  // A fact shows as a sentence: its "edit" button stands for every field.
  const fact = /^knowledge\[\d+\]/.exec(target);
  return fact ? fact[0] : target;
}

/** The element id of the input for a field path. */
export function fieldId(path: string): string {
  return `f-${path.replace(/[^A-Za-z0-9_]/g, "-")}`;
}

const quote = (s: string) => `“${s}”`;
const PC_FIELDS: Record<string, string> = {
  name: "Your character's name",
  age: "Your character's age",
  concept: "Your character's concept",
};
const FACT_FIELDS: Record<string, string> = { subject: "About", key: "Detail", value: "Value" };

/** The sentence of known fact `i`, quoted, or its number when it has none. */
function factName(form: ReviewForm, i: number): string {
  const text = form.knowledge[i]?.text.trim();
  return text ? `the fact ${quote(text)}` : `fact ${i + 1}`;
}

const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** The field at `at`, in plain words, starting with a capital letter. No internal field name. */
export function fieldLabel(at: string, form: ReviewForm): string {
  const { seed } = form;
  const p = parseAt(at);
  const [a, b, c, d, e, f] = p;
  const num = (n: unknown) => (typeof n === "number" ? n : -1);
  const entityName = (i: number) => {
    const name = seed.entities[i]?.name.trim();
    return name ? name : `entry ${i + 1}`;
  };
  const owner = a === "pc" ? "your character" : entityName(num(b));
  const otherNames = a === "pc" ? seed.pc.other_names : seed.entities[num(b)]?.other_names;
  const otherName = (j: number) => {
    const name = otherNames?.[j]?.name.trim();
    return name ? `The other name ${quote(name)} of ${owner}` : `Other name ${j + 1} of ${owner}`;
  };
  const holderOf = (j: number, k: number) => {
    const other = otherNames?.[j];
    const holder = other?.known_to[k]?.trim() || `name ${k + 1}`;
    const name = other?.name.trim() || `other name ${j + 1}`;
    return `${quote(holder)} in who knows ${quote(name)}`;
  };

  if (p.length === 1) {
    const top: Record<string, string> = {
      premise: "The premise",
      story_language: "The story language",
      seed: "The story setup",
      min_age: "The minimum age",
      gm_agreement: "How the GM runs the game",
      resolution: "How actions are resolved",
    };
    if (typeof a === "string" && Object.hasOwn(top, a)) return top[a] ?? at;
  }
  if (a === "pc") {
    if (p.length === 2 && typeof b === "string" && Object.hasOwn(PC_FIELDS, b)) {
      return PC_FIELDS[b] ?? at;
    }
    if (b === "other_names" && p.length === 3) return otherName(num(c));
    if (b === "other_names" && d === "known_to" && p.length === 5) return holderOf(num(c), num(e));
  }
  if (a === "entities" && typeof b === "number") {
    if (p.length === 3 && (c === "name" || c === "kind" || c === "age")) {
      return `The ${c} of ${entityName(b)}`;
    }
    if (c === "other_names" && p.length === 4) return otherName(num(d));
    if (c === "other_names" && e === "known_to" && p.length === 6) return holderOf(num(d), num(f));
  }
  if (a === "knowledge" && typeof b === "number") {
    if (p.length === 2) return capital(factName(form, b));
    if (p.length === 3 && c === "text") return `The sentence of fact ${b + 1}`;
    if (p.length === 3 && typeof c === "string" && Object.hasOwn(FACT_FIELDS, c)) {
      return `${quote(FACT_FIELDS[c] ?? c)} in ${factName(form, b)}`;
    }
  }
  if (a === "start" && p.length === 2 && b === "place") return "The start place";
  if (a === "start" && b === "present" && p.length === 3) {
    const name = seed.start.present[num(c)]?.trim() || `name ${num(c) + 1}`;
    return `${quote(name)} in ${quote("who is present")}`;
  }
  if ((a === "decided" || a === "proposed") && typeof b === "number" && p.length === 3) {
    return `${a === "decided" ? "Decided" : "Proposed"} fact ${b + 1}`;
  }
  return at;
}

/** What to do about a name that matches no entry, by the field that holds it. */
function unknownName(at: string, field: string, form: ReviewForm): string {
  const p = parseAt(at);
  if (at === "start.place") {
    return `${field} is not a place of the draft. Use the name of a place below, or add the place.`;
  }
  if (p[0] === "start" && p[1] === "present") {
    return `${field} is not a character of the draft. Use a character's name, or add the character.`;
  }
  if (p.includes("known_to")) {
    return `${field} is not a character of the draft. Use a character's name, or delete it.`;
  }
  if (p[0] === "knowledge" && typeof p[1] === "number") {
    const about = form.knowledge[p[1]]?.subject.trim() ?? "";
    return `${capital(factName(form, p[1]))} is about ${quote(about)}, who is not in the draft. Press “edit” and change “About”, or add ${quote(about)} as a character or a place.`;
  }
  return `${field} does not match a name of the draft. Use a name from the draft, or add the entry.`;
}

/** What to do about a field its entry may not have, by the field. */
function wrongKind(at: string, field: string, form: ReviewForm): string {
  const p = parseAt(at);
  if (p.at(-1) === "age") return `${field} is set, but only a character has an age. Clear it.`;
  if (p.at(-1) === "kind") {
    return `${field} is “your character”, and the draft already has one. Choose another kind.`;
  }
  if (p[0] === "knowledge" && typeof p[1] === "number") {
    return `${capital(factName(form, p[1]))} has “pending” in its detail. Press “edit”, remove “pending” from “Detail”, and tick “Not done yet” instead.`;
  }
  return `${field} is not allowed here. Change it.`;
}

/** One sentence for a gap: what is wrong, then what to do. */
export function gapSentence(gap: Gap, form: ReviewForm): string {
  const field = fieldLabel(gap.at, form);
  const min = form.seed.min_age;
  const sentences: Record<GapKind, () => string> = {
    missing: () => `${field} is empty. Write it in.`,
    age_missing: () => `${field} is missing. Enter an age in years.`,
    age_unconfirmed: () => `${field} is not confirmed yet. Check it, then press “Confirm age”.`,
    age_below_min: () =>
      `${field} is below the minimum age${min === null ? "" : ` of ${min}`}. Raise the age, or change the minimum age under “How you want to play”.`,
    unknown_name: () => unknownName(gap.at, field, form),
    duplicate_name: () => `${field} is used twice. Give one of them another name.`,
    pc_in_roster: () => `${field} is a name of your character. Delete this entry, or give it another name.`,
    wrong_kind: () => wrongKind(gap.at, field, form),
    seed_block_too_long: () =>
      "The story setup is too long for the GM to read. Shorten the premise, the GM agreement or the content limits.",
    slot_conflict: () =>
      `${field} says something about the same thing as another fact. Delete one of them, or merge them.`,
  };
  return Object.hasOwn(sentences, gap.kind)
    ? sentences[gap.kind]()
    : `${field} needs a fix. Check it.`;
}

/** The parts of the review, in order: a guided path through the draft. */
export type SectionId = "story" | "you" | "people" | "places" | "knows" | "open" | "play";

export const SECTIONS: readonly (readonly [SectionId, string])[] = [
  ["story", "The story"],
  ["you", "Your character"],
  ["people", "People"],
  ["places", "Places and groups"],
  ["knows", "What your character knows"],
  ["open", "Open matters"],
  ["play", "How you want to play"],
];

/** The part of the review that holds the field at `at`. */
export function sectionOf(at: string, form: ReviewForm): SectionId {
  const [a, b] = parseAt(at);
  if (a === "pc") return "you";
  if (a === "entities") {
    const entry = typeof b === "number" ? form.seed.entities[b] : undefined;
    return entry && isCharacter(entry.kind) ? "people" : entry ? "places" : "people";
  }
  if (a === "knowledge") {
    return typeof b === "number" && form.knowledge[b]?.open ? "open" : "knows";
  }
  if (a === "open_details") return "open";
  if (a === "themes" || a === "boundaries" || a === "gm_agreement" || a === "resolution" || a === "min_age") {
    return "play";
  }
  return "story";
}

/** The gaps of the field at `path`, and of the fields inside it. */
export function gapsAt(gaps: Gap[], path: string): Gap[] {
  return gaps.filter(
    (g) => g.at === path || g.at.startsWith(`${path}.`) || g.at.startsWith(`${path}[`),
  );
}

/** One sentence for a list the app cut at its cap: what was cut, and what it means. */
export function cutSentence(cut: Cut, form: ReviewForm): string {
  const p = parseAt(cut.at);
  const n = cut.cap;
  if (cut.at === "entities") {
    return `Your document names more than ${n} people, or more than ${n} places and groups. The draft kept the first ${n} of each: add any missing one with “Add a character” or “Add a place”.`;
  }
  if (cut.at === "knowledge") {
    return `A part of your document holds more than ${n} facts. The draft kept the first ${n} of that part, so some facts may be missing.`;
  }
  if (cut.at === "themes") return `Your document gives more than ${n} themes. The draft kept the first ${n}.`;
  if (cut.at === "open_details") {
    return `Your document leaves more than ${n} things open. The draft kept the first ${n}.`;
  }
  if (cut.at === "start.present") {
    return `More than ${n} people are present at the start. The draft kept the first ${n}.`;
  }
  if (p.at(-1) === "other_names") {
    const owner = p[0] === "pc" ? "Your character" : (form.seed.entities[Number(p[1])]?.name ?? "An entry");
    return `${owner} has more than ${n} other names in your document. The draft kept the first ${n}.`;
  }
  if (p.at(-1) === "known_to") {
    return `More than ${n} people know one of the other names. The draft kept the first ${n}.`;
  }
  return `A list of the draft was cut at ${n} items.`;
}

/** The parts of the review where a cut list shows. */
export function cutSections(cut: Cut, form: ReviewForm): SectionId[] {
  if (cut.at === "entities") return ["people", "places"];
  if (cut.at === "knowledge") return ["knows", "open"];
  return [sectionOf(cut.at, form)];
}

/**
 * Where a saved draft opens. Only an approved draft with no gap goes to the
 * World Build: an old file can say "approved" and still wait for an age.
 */
export function draftScreen(view: DraftView): "review" | "build" {
  return view.state === "approved" && view.gaps.length === 0 ? "build" : "review";
}

/** The review form of a draft as the store holds it. */
export function formOfView(view: DraftView): ReviewForm {
  return { seed: view.seed, knowledge: view.knowledge, typed: {} };
}

/** What `save_draft` receives: never a typed name, never an age list. */
export function saveArgs(form: ReviewForm): { seed: AdventureSeed; knowledge: KnownFact[] } {
  return { seed: form.seed, knowledge: form.knowledge };
}

/** The key of a row in `typed`, in the path form of a gap. */
export const rowPath = (row: Row): string => (row === "pc" ? "pc" : `entities[${row}]`);

/** The saved main name of a row. */
export const savedName = (form: ReviewForm, row: Row): string =>
  row === "pc" ? form.seed.pc.name : (form.seed.entities[row]?.name ?? "");

/** The entry of a row as the core names it: its row, its saved main name and its kind. */
export function entryOf(form: ReviewForm, row: Row): EntryRef {
  if (row === "pc") return { index: null, name: form.seed.pc.name, kind: "pc" };
  const entry = form.seed.entities[row];
  return { index: row, name: entry?.name ?? "", kind: entry?.kind ?? "npc" };
}

/** The name the field shows: what the Player is typing, else the saved name. */
export const shownName = (form: ReviewForm, row: Row): string =>
  form.typed[rowPath(row)] ?? savedName(form, row);

/** The Player types in a name field. The seed does not change. */
export function typeName(form: ReviewForm, row: Row, text: string): ReviewForm {
  return { ...form, typed: { ...form.typed, [rowPath(row)]: text } };
}

/** The Player leaves a name field without a change (Esc). */
export function dropTyped(form: ReviewForm, row: Row): ReviewForm {
  const key = rowPath(row);
  const typed = Object.fromEntries(Object.entries(form.typed).filter(([k]) => k !== key));
  return { ...form, typed };
}

/**
 * How a typed name commits. A saved name goes through the core's rename,
 * which follows every reference. A row with no saved name has nothing to
 * follow: the name is saved as an edit, and its age needs a confirmation.
 */
export type NameChange =
  | { kind: "none" }
  | { kind: "rename"; entry: EntryRef; to: string }
  | { kind: "save"; to: string };

export function nameChange(form: ReviewForm, row: Row): NameChange {
  const typed = form.typed[rowPath(row)];
  if (typed === undefined) return { kind: "none" };
  const from = savedName(form, row);
  const to = typed.trim();
  if (to === from) return { kind: "none" };
  if (from.trim() === "") return { kind: "save", to };
  return { kind: "rename", entry: entryOf(form, row), to };
}

/**
 * The form after the core renamed a row: the seed and knowledge come back
 * from the store, every reference renamed. Other typed names stay.
 */
export function renamed(form: ReviewForm, view: DraftView, row: Row): ReviewForm {
  return { ...dropTyped(form, row), seed: view.seed, knowledge: view.knowledge };
}

/** True for the kinds that have an age. */
export const isCharacter = (kind: EntityKind): boolean => kind === "npc" || kind === "pc";

/** The core's age line of a row, while it is about the entry the row holds; none for a row that is not a character. */
export function ageLineOf(ages: AgeLine[], form: ReviewForm, row: Row): AgeLine | undefined {
  const entry = entryOf(form, row);
  return ages.find(
    (line) => line.index === entry.index && line.name === entry.name && line.kind === entry.kind,
  );
}

/**
 * An age typed as text: whole years, in decimal digits only. `null` for an
 * empty field; `undefined` for any other text ("0x10", "1e1", "16.5", "-3").
 */
export function parseAge(text: string): number | null | undefined {
  const t = text.trim();
  if (t === "") return null;
  if (!/^\d{1,5}$/.test(t)) return undefined;
  const n = Number(t);
  return n <= 65535 ? n : undefined;
}

/** The start hour typed in the review: null when empty (the core then starts at 8), undefined when it is not a whole hour 0 to 23. */
export function parseHour(text: string): number | null | undefined {
  const t = text.trim();
  if (t === "") return null;
  if (!/^\d{1,2}$/.test(t)) return undefined;
  const n = Number(t);
  return n <= 23 ? n : undefined;
}

/** True when an age field shows the age the form holds. `text` is null while the field shows that age. */
export const ageTextMatches = (text: string | null, value: number | null): boolean =>
  value !== null && (text === null || text === String(value));

export interface AgeField {
  /** The text in the field; null while the field shows the stored age. */
  text: string | null;
  /** The age the form holds for the row. */
  value: number | null;
  /** The core's line for the row. */
  line: AgeLine | undefined;
  /** A save of the draft waits or runs. */
  pending: boolean;
  /** The last save of the draft failed. */
  saveFailed: boolean;
  /** A core action runs. */
  busy: boolean;
}

/**
 * What an age row shows. "confirmed" shows only while the field shows the
 * very age the store holds and the core confirmed: it drops at once when
 * the field changes, while a save waits, and after a failed save. "Confirm
 * age" works only in that same settled state, so it never confirms a value
 * the field does not show.
 */
export function ageStatus(f: AgeField): { confirmed: boolean; canConfirm: boolean } {
  const settled = ageTextMatches(f.text, f.value) && !f.pending && !f.saveFailed;
  const confirmed = settled && f.line?.confirmed === true;
  return { confirmed, canConfirm: settled && !confirmed && !f.busy };
}

export interface ApproveState {
  pending: boolean;
  busy: boolean;
  approving: boolean;
  saveFailed: boolean;
  /** An error the review shows. */
  error: string | null;
  gaps: Gap[];
  state: DraftState;
  /** Every age field shows the age the form holds. */
  agesShown: boolean;
}

/** "Approve" waits for the store to hold every edit, for no gap, and for each age field to show its age. */
export const canApprove = (a: ApproveState): boolean =>
  !a.pending &&
  !a.busy &&
  !a.approving &&
  !a.saveFailed &&
  a.error === null &&
  a.gaps.length === 0 &&
  a.agesShown &&
  a.state !== "built";

/** A core action on the draft. The store first takes every pending edit, so the core acts on what the Player sees. */
export async function afterFlush<T>(flush: () => Promise<void>, call: () => Promise<T>): Promise<T> {
  await flush();
  return call();
}

/** "Confirm age" on a row: the store takes every edit, then the core confirms that exact entry. */
export function confirmRow<T>(
  flush: () => Promise<void>,
  confirm: (entry: EntryRef) => Promise<T>,
  form: ReviewForm,
  row: Row,
): Promise<T> {
  const entry = entryOf(form, row);
  return afterFlush(flush, () => confirm(entry));
}

/**
 * Where an age comes from, in the Player's words. "from the text" stays only
 * on an untouched value the text states; any value the Player changed or
 * added says "edited by you".
 */
export function ageLabel(line: AgeLine | undefined): string {
  if (!line || line.source === "edited") return "edited by you";
  return line.source === "from_text" ? "from the text" : "proposed by the model";
}

/** How many ages still wait for the Player's confirmation. */
export const agesLeft = (ages: AgeLine[]): number => ages.filter((l) => !l.confirmed).length;

/** The form with one more roster entry, as "Add a character" or "Add a place" makes it. */
export function addEntry(form: ReviewForm, entry: SeedEntity): ReviewForm {
  return { ...form, seed: { ...form.seed, entities: [...form.seed.entities, entry] } };
}

/** The form without the roster entry at `i`. Typed names of the roster are dropped: their rows move. */
export function removeEntry(form: ReviewForm, i: number): ReviewForm {
  const typed = form.typed.pc === undefined ? {} : { pc: form.typed.pc };
  return { ...form, seed: { ...form.seed, entities: removeAt(form.seed.entities, i) }, typed };
}

/** `list` with the item at `i` patched. */
export function replaceAt<T>(list: T[], i: number, patch: Partial<T>): T[] {
  return list.map((item, j) => (j === i ? { ...item, ...patch } : item));
}

/** `list` without the item at `i`. */
export function removeAt<T>(list: T[], i: number): T[] {
  return list.filter((_, j) => j !== i);
}

/** The items of a list typed as text, trimmed, without the empty ones. */
export function splitList(text: string, separator: "," | "\n"): string[] {
  return text
    .split(separator)
    .map((s) => s.trim())
    .filter(Boolean);
}

export interface SaverOptions<F, R> {
  save: (form: F) => Promise<R>;
  /** How long typing must pause before a save. */
  delayMs: number;
  /** The result of the latest form only: a stale result is dropped. */
  onResult: (result: R) => void;
  onError: (e: unknown) => void;
  /** True from an edit until the store holds it. */
  onPending: (pending: boolean) => void;
}

export interface Saver<F> {
  /** A new form; `now` skips the pause (a click, not typing). */
  edit(form: F, now?: boolean): void;
  /** Saves the last edit at once and reports nothing more. */
  stop(): void;
  /** Takes edits and reports again after `stop()`. */
  start(): void;
  /**
   * Saves the last edit now and waits until the store holds every edit. A
   * form whose save failed earlier is sent once more. Rejects when the store
   * still does not hold the latest form.
   */
  flush(): Promise<void>;
}

type Outcome = { ok: true } | { ok: false; error: unknown };

/**
 * Saves a form after typing pauses. One save runs at a time, so two saves
 * never race in the store; edits made meanwhile coalesce into one next save.
 */
export function createSaver<F, R>(opts: SaverOptions<F, R>): Saver<F> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let waiting: { form: F } | null = null;
  let inFlight = false;
  let pending = false;
  let stopped = false;
  // The running save, and after it any save it starts; null when idle.
  let current: Promise<Outcome> | null = null;
  // The latest form the store refused; cleared when a later save succeeds.
  let unsaved: { form: F } | null = null;

  const settle = () => {
    const now = timer !== null || inFlight || waiting !== null;
    if (now !== pending && !stopped) opts.onPending(now);
    pending = now;
  };

  const send = () => {
    if (inFlight || waiting === null || (timer !== null && !stopped)) return settle();
    const { form } = waiting;
    waiting = null;
    inFlight = true;
    settle();
    const done: Promise<Outcome> = opts
      .save(form)
      .then(
        (result): Outcome => {
          unsaved = null;
          if (!stopped && waiting === null) opts.onResult(result);
          return { ok: true };
        },
        (error: unknown): Outcome => {
          unsaved = { form };
          if (!stopped) opts.onError(error);
          return { ok: false, error };
        },
      )
      .then((outcome) => {
        inFlight = false;
        if (current === done) current = null;
        send();
        return outcome;
      });
    current = done;
  };

  const clearTimer = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  return {
    edit(form, now = false) {
      if (stopped) return;
      waiting = { form };
      clearTimer();
      if (!now) {
        timer = setTimeout(() => {
          timer = null;
          send();
        }, opts.delayMs);
      }
      send();
    },
    stop() {
      stopped = true;
      clearTimer();
      send();
    },
    start() {
      stopped = false;
    },
    async flush() {
      const drain = async () => {
        send();
        let last: Outcome = { ok: true };
        while (current) last = await current;
        if (!last.ok) throw last.error;
      };
      clearTimer();
      await drain();
      // An earlier save failed and nothing newer replaced it: one more try.
      if (unsaved) {
        waiting = unsaved;
        await drain();
      }
    },
  };
}
