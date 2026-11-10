// Pure helpers of the New Campaign screen: labels, gap sentences, locators,
// name matching and the draft saver.

import type {
  AdventureSeed,
  BuildFailure,
  BuildProgress,
  BuildStage,
  DraftView,
  Gap,
  GapKind,
  KnownFact,
} from "./api";

/**
 * One flag per character row. It is kept per row, never by name: a name the
 * Player is typing must not move a mark.
 */
export interface AgeFlags {
  pc: boolean;
  /** One flag per `seed.entities` entry, in the same order. */
  entities: boolean[];
}

/**
 * What the review edits. `save_draft` takes it, with `agesToConfirm` and
 * `agesStated` as the name lists.
 */
export interface ReviewForm {
  seed: AdventureSeed;
  knowledge: KnownFact[];
  /** The rows whose age the Player has not confirmed. A new draft marks every character. */
  unconfirmed: AgeFlags;
  /** The rows whose age the model says the text states. Information only. */
  stated: AgeFlags;
}

/** A character row: the PC, or an index in `seed.entities`. */
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
  if (/^knowledge\[\d+\]$/.test(target)) target += ".text";
  return target;
}

/** The element id of the input for a field path. */
export function fieldId(path: string): string {
  return `f-${path.replace(/[^A-Za-z0-9_]/g, "-")}`;
}

const quote = (s: string) => `“${s}”`;
const ENTITY_FIELDS: Record<string, string> = { name: "name", kind: "kind", age: "age" };
const PC_FIELDS: Record<string, string> = { name: "name", age: "age", concept: "concept" };

/** The field at `at`, in plain words, starting with a capital letter. */
export function fieldLabel(at: string, form: ReviewForm): string {
  const { seed } = form;
  const p = parseAt(at);
  const [a, b, c, d, e, f] = p;
  const num = (n: unknown) => (typeof n === "number" ? n : -1);
  const entityName = (i: number) => {
    const name = seed.entities[i]?.name.trim();
    return name ? name : `entry ${i + 1}`;
  };
  const otherNames = a === "pc" ? seed.pc.other_names : seed.entities[num(b)]?.other_names;
  const holderOf = (j: number, k: number) => {
    const other = otherNames?.[j];
    const holder = other?.known_to[k]?.trim() || `name ${k + 1}`;
    const name = other?.name.trim() || `other name ${j + 1}`;
    return `${quote(holder)} in who knows ${quote(name)}`;
  };

  if (p.length === 1 && a === "premise") return "The premise";
  if (p.length === 1 && a === "story_language") return "The story language";
  if (p.length === 1 && a === "seed") return "The seed";
  if (a === "pc") {
    if (p.length === 2 && typeof b === "string" && Object.hasOwn(PC_FIELDS, b)) {
      return `Your character's ${PC_FIELDS[b]}`;
    }
    if (b === "other_names" && p.length === 3) return `Your character's other name ${num(c) + 1}`;
    if (b === "other_names" && d === "known_to" && p.length === 5) return holderOf(num(c), num(e));
  }
  if (a === "entities" && typeof b === "number") {
    if (p.length === 3 && typeof c === "string" && Object.hasOwn(ENTITY_FIELDS, c)) {
      return `The ${ENTITY_FIELDS[c]} of ${entityName(b)}`;
    }
    if (c === "other_names" && p.length === 4) {
      return `Other name ${num(d) + 1} of ${entityName(b)}`;
    }
    if (c === "other_names" && e === "known_to" && p.length === 6) return holderOf(num(d), num(f));
  }
  if (a === "knowledge" && typeof b === "number") {
    if (p.length === 2) return `Known fact ${b + 1}`;
    if (p.length === 3 && typeof c === "string") return `The ${c} of known fact ${b + 1}`;
  }
  if (a === "start" && p.length === 2 && b === "place") return "The start place";
  if (a === "start" && b === "present" && p.length === 3) {
    const name = seed.start.present[num(c)]?.trim() || `name ${num(c) + 1}`;
    return `${quote(name)} in ${quote("who is present")}`;
  }
  if ((a === "decided" || a === "proposed") && typeof b === "number" && p.length === 3) {
    return `${a === "decided" ? "Decided" : "Proposed"} fact ${b + 1} (${String(c)})`;
  }
  return at;
}

const SENTENCES: Record<GapKind, (field: string) => string> = {
  missing: (f) => `${f} is empty.`,
  age_missing: (f) => `${f} is missing.`,
  age_unconfirmed: (f) => `${f} is not confirmed: check it, then confirm it.`,
  age_below_min: (f) => `${f} is below the minimum age.`,
  unknown_name: (f) => `${f} does not match a name of the right kind.`,
  duplicate_name: (f) => `${f} is used twice.`,
  pc_in_roster: (f) => `${f} is a name of your character.`,
  wrong_kind: (f) => `${f} is not allowed here.`,
  seed_block_too_long: () => "The seed is too long: shorten some text.",
  slot_conflict: (f) => `${f} repeats another fact on the same thing.`,
};

/** One plain sentence for a gap. */
export function gapSentence(gap: Gap, form: ReviewForm): string {
  const field = fieldLabel(gap.at, form);
  return Object.hasOwn(SENTENCES, gap.kind) ? SENTENCES[gap.kind](field) : `${field} needs a fix.`;
}

/** A character that a name drops at its ends: anything not a letter or digit. */
const EDGES = /^[^\p{Alphabetic}\p{N}]+|[^\p{Alphabetic}\p{N}]+$/gu;

/** The normal form of a name, as `normalize::name` in the core makes it. */
export function nameKey(name: string): string {
  const lowered = name.normalize("NFKC").toLowerCase().replace(/[‘’]/g, "'");
  const collapsed = lowered.replace(EDGES, "").split(/\s+/).filter(Boolean).join(" ");
  const article = ["the ", "a ", "an "].find((a) => collapsed.startsWith(a));
  const bare = article ? collapsed.slice(article.length) : collapsed;
  return bare
    .replace(EDGES, "")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .normalize("NFC");
}

/** The rows whose name is in the draft's `ages_to_confirm`, matched as the core matches. */
export function flagsFromNames(seed: AdventureSeed, names: string[]): AgeFlags {
  const keys = new Set(names.map(nameKey));
  return {
    pc: keys.has(nameKey(seed.pc.name)),
    entities: seed.entities.map((e) => keys.has(nameKey(e.name))),
  };
}

/** The review form of a draft as the store holds it. */
export function formOfView(view: DraftView): ReviewForm {
  return {
    seed: view.seed,
    knowledge: view.knowledge,
    unconfirmed: flagsFromNames(view.seed, view.ages_to_confirm),
    stated: flagsFromNames(view.seed, view.ages_stated),
  };
}

const rowName = (form: ReviewForm, row: Row) =>
  row === "pc" ? form.seed.pc.name : (form.seed.entities[row]?.name ?? "");

/**
 * The `ages_to_confirm` list for `save_draft`: the current names of the
 * unconfirmed rows, built only when saving.
 */
export function agesToConfirm(form: ReviewForm): string[] {
  return rowNames(form, form.unconfirmed);
}

/** The `ages_stated` list for `save_draft`: the current names of the rows the model calls stated. */
export function agesStated(form: ReviewForm): string[] {
  return rowNames(form, form.stated);
}

/** Where a row's age comes from, as the model said it. It never confirms the age. */
export function ageSource(form: ReviewForm, row: Row): "from the text" | "proposed by the model" {
  const stated = row === "pc" ? form.stated.pc : form.stated.entities[row];
  return stated ? "from the text" : "proposed by the model";
}

/** The current names of the flagged rows, each name once. */
function rowNames(form: ReviewForm, flags: AgeFlags): string[] {
  const rows: Row[] = [
    ...(flags.pc ? (["pc"] as const) : []),
    ...flags.entities.flatMap((on, i) => (on ? [i] : [])),
  ];
  const seen = new Set<string>();
  const names: string[] = [];
  for (const row of rows) {
    const name = rowName(form, row);
    if (seen.has(nameKey(name))) continue;
    seen.add(nameKey(name));
    names.push(name);
  }
  return names;
}

/**
 * True while the row's age is unconfirmed. The store matches by name, so a
 * row that shares its name with an unconfirmed row is unconfirmed too.
 */
export function isUnconfirmed(form: ReviewForm, row: Row): boolean {
  const own = row === "pc" ? form.unconfirmed.pc : form.unconfirmed.entities[row];
  if (own) return true;
  const key = nameKey(rowName(form, row));
  return key !== "" && agesToConfirm(form).some((n) => nameKey(n) === key);
}

/** The Player confirms the age of one row; no other row changes. */
export function confirmRow(form: ReviewForm, row: Row): ReviewForm {
  const unconfirmed =
    row === "pc"
      ? { ...form.unconfirmed, pc: false }
      : { ...form.unconfirmed, entities: replaceFlag(form.unconfirmed.entities, row) };
  return { ...form, unconfirmed };
}

const replaceFlag = (flags: boolean[], i: number) => flags.map((on, j) => (j === i ? false : on));

/** The form without the roster entry at `i`, and without that entry's flags only. */
export function removeEntityRow(form: ReviewForm, i: number): ReviewForm {
  return {
    ...form,
    seed: { ...form.seed, entities: removeAt(form.seed.entities, i) },
    unconfirmed: { ...form.unconfirmed, entities: removeAt(form.unconfirmed.entities, i) },
    stated: { ...form.stated, entities: removeAt(form.stated.entities, i) },
  };
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
