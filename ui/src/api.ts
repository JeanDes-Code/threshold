// Typed access to the Tauri commands and events of the shell (src-tauri).
// Field names mirror the Rust types: snake_case.

export type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export interface RollResult {
  seed: number;
  difficulty: number;
  value: number;
  success: boolean;
}

/** The Campaign just opened. Its story comes from `get_play_view`. */
export interface CampaignSummary {
  db_path: string;
  pc_name: string;
  premise: string;
  /** No Opening Turn and no play Turn yet: "Begin" is still due. */
  needs_begin: boolean;
}

/**
 * One folder of `<data_dir>/packs` on the home menu: a loaded Content Pack by
 * its name, or a refused one by its folder and the core's sentence for the
 * rule it broke. It never holds a line of a pack.
 */
export interface PackLine {
  folder: string;
  name: string | null;
  /** The pack declares that it gives explicit description. */
  explicit: boolean;
  /** Shown, never applied: the Player chooses the model. */
  recommended_model: string | null;
  problem: string | null;
}

/** One Campaign file on the home screen. A card with a `problem` cannot open. */
export interface CampaignCard {
  /** The card's own id (its file name). "Remove from list" sends it, never a path. */
  id: string;
  db_path: string;
  pc_name: string;
  premise: string;
  /** The change time of the file, in ms since the epoch. */
  last_played_ms: number;
  /** The Play and Correction Turns that stand. */
  turn_count: number;
  needs_begin: boolean;
  problem: string | null;
}

/** A committed Turn. `narration` is null when the narration failed after the Commit. */
export interface TurnSummary {
  turn_id: number;
  roll: RollResult | null;
  narration: string | null;
  narration_error: string | null;
  total_ms: number;
  /** The Player faded the Narration: `narration` holds its neutral line. */
  faded: boolean;
}

/** What `fade_to_black` did: `pending` while a Turn was running, which finishes the fade itself. */
export interface FadeResult {
  pending: boolean;
}

export interface CharacterView {
  name: string;
  concept: string;
  clock_min: number;
  /** What the PC knows about itself: no open item and no outcome of one. */
  facts: CharacterFact[];
  /** The Open Items the PC knows, as the side panel and the Journal list them. */
  open: FactLine[];
  /** The state lines of the "You" card: condition, carrying, mood… */
  state: StateLine[];
}

/** One thing the PC knows about itself. */
export interface CharacterFact {
  text: string;
  /** Who told it; null for what the PC saw, and for the PC's own decision. */
  told_by: NameRef | null;
}

export interface DebugLine {
  step: string;
  prompt: string;
  raw: string;
  ms: number;
}

export interface DebugView {
  turn_id: number;
  op_id: string;
  kind: string;
  intent: string;
  proposal_json: string;
  narration: string | null;
  entries: DebugLine[];
}

// The Play view: a mirror of shape 3.6 and 3.7. Every tagged enum is tagged
// by `kind`. The core builds it from PC Knowledge only; the UI parses no text.

export type TurnKind = "setup" | "play" | "correction" | "ooc" | "opening" | "closing";

/** A person, place or Faction as the PC knows it. `name` is null when the PC cannot name it. */
export interface NameRef {
  entity_id: number;
  kind: EntityKind;
  name: string | null;
}

export interface PlaceLine {
  entity_id: number | null;
  name: string;
}

export type Part = { kind: "text"; text: string } | { kind: "name"; entity_id: number; text: string };

export type Block =
  | { kind: "prose"; parts: Part[] }
  | {
      kind: "speech";
      speaker: NameRef;
      parts: Part[];
      /** The stage note written before the words, without its parentheses: a quiet line. */
      stage: Part[] | null;
    };

/** `faded`: the Player faded the Narration; the core's neutral line stands in its place. */
export type TurnBody =
  | { kind: "narration"; blocks: Block[] }
  | { kind: "missing" }
  | { kind: "faded"; line: string };

export interface StoryTurn {
  turn_id: number;
  kind: TurnKind;
  intent: string;
  roll: RollResult | null;
  /** A later Correction reverted this Turn. Its body holds prose blocks only. */
  reverted: boolean;
  body: TurnBody;
  /** What the PC found in this Turn: a small card list under the Narration. */
  found: FoundCard | null;
}

/** What the PC found in one Turn: where it comes from, and one row per item. */
export interface FoundCard {
  source: string;
  items: FoundRow[];
}

/** One found item: a short title and one line of what it says or shows. */
export interface FoundRow {
  title: string;
  line: string;
}

/** What one Turn of a Scene found, for the Journal. */
export interface FoundEntry {
  turn_id: number;
  source: string;
  items: FoundRow[];
}

/** A PC Belief about the PC on a state key (never `location`). */
export interface StateLine {
  key: string;
  value: string;
  /** The line as the Player reads it: a plain sentence. */
  text: string;
}

export interface YouCard {
  name: string;
  concept: string;
  state: StateLine[];
}

/** A token for one Belief. The UI does not show it. */
export interface BeliefRef {
  subject_id: number;
  key: string;
}

export interface FactLine {
  belief: BeliefRef;
  subject: NameRef;
  text: string;
  /** An Open Item: planned or pending, not done. */
  open: boolean;
  told_by: NameRef | null;
}

/** A named NPC, place or Faction the PC knows. */
export interface KnownLine {
  who: NameRef;
  other_names: string[];
  here: boolean;
}

export type Target =
  | { kind: "scene"; turn_id: number }
  | { kind: "act"; turn_id: number }
  | { kind: "belief"; subject_id: number; key: string }
  | { kind: "entity"; entity_id: number }
  | { kind: "note"; note_id: number };

export interface PinLine {
  target: Target;
  title: string;
  text: string;
}

/** Text the Player writes in the Journal. Never part of the world. */
export interface Note {
  id: number;
  body: string;
  about: Target | null;
  created_ms: number;
  updated_ms: number;
}

/** What the PC did in one Turn: its Intent. */
export interface Act {
  turn_id: number;
  /** Empty when the Turn was faded: show `faded` instead. */
  intent: string;
  /** The neutral line of a faded Turn; null otherwise. */
  faded: string | null;
  /** A decision of the PC: it lasts, and the PC saw it. */
  decision: boolean;
  /** The Open Items this Turn closed, each with how it ended. */
  closed: ClosedLine[];
}

/** How one Open Item ended: "Refused" and the item's text. */
export interface ClosedLine {
  /** "Accepted", "Refused", "Done", "Failed", "Dropped", or a value a public event wrote. */
  outcome: string;
  text: string;
}

/** One Scene: one place and time, who was there, what the PC did and learned. */
export interface SceneEntry {
  /** The first Turn of the Scene. */
  turn_id: number;
  /** The place name; empty when the PC has no location. */
  place: string;
  clock_from: number;
  clock_to: number;
  /** Who was there, the PC left out. */
  present: NameRef[];
  acts: Act[];
  learned: FactLine[];
  /** What the PC found in the Scene's Turns, oldest first. */
  found: FoundEntry[];
}

export interface ClosedItem {
  item: FactLine;
  closed_in: number;
  /** How the item ended, as one word: "Refused". */
  outcome: string;
  /** The first Turn of the Scene that closed it, when that Turn stands. */
  scene: number | null;
}

/** The Journal: written by the app from PC Knowledge only. */
export interface JournalView {
  /** Oldest first. */
  scenes: SceneEntry[];
  open: FactLine[];
  closed: ClosedItem[];
  /** What the PC knew before the Campaign began. */
  known_before: FactLine[];
  notes: Note[];
  pins: Target[];
}

/** One person, place or Faction as the PC knows it. */
export interface EntityCard {
  who: NameRef;
  other_names: string[];
  /** Present now, as the PC perceives it. */
  here: boolean;
  /** What the PC knows about it, Open Items marked. */
  facts: FactLine[];
  /** A place only: who the PC believes is located there. A name is null when the PC cannot name them. */
  people_here: NameRef[];
  /** A character the PC saw leave for a place it cannot name: "Left; you do not know where." Null otherwise. */
  whereabouts: string | null;
  /** A place or a Faction only: facts the PC holds about others that name it. */
  named_in: FactLine[];
  notes: Note[];
  pinned: boolean;
}

/** The scene the Play screen is in: plain, or an Intimate Scene open
 * since a Turn (data shape 3.11). The action bar reads the tag. */
export type SceneState = { kind: "plain" } | { kind: "intimate"; since_turn: number };

/**
 * The Detail Level the Campaign stores, and the one it plays at with the
 * loaded Content Packs (data shape 3.11).
 */
export interface DetailView {
  stored: DetailLevel;
  effective: DetailLevel;
}

export interface PlayView {
  pc_name: string;
  place: PlaceLine | null;
  clock_min: number;
  needs_begin: boolean;
  can_correct: boolean;
  scene: SceneState;
  detail: DetailView;
  story: StoryTurn[];
  here: NameRef[];
  you: YouCard;
  open: FactLine[];
  pinned: PinLine[];
  known: KnownLine[];
}

export interface Defaults {
  seed_path: string;
  world_path: string;
  data_dir: string;
}

export type EntityKind = "pc" | "npc" | "faction" | "place" | "item";

export interface OtherName {
  name: string;
  /** The characters who know that this name names the same entity. */
  known_to: string[];
}

export interface PcSeed {
  name: string;
  age: number;
  other_names: OtherName[];
  concept: string;
}

export interface SeedEntity {
  name: string;
  kind: EntityKind;
  /** A character's age in years; null for anything else. */
  age: number | null;
  other_names: OtherName[];
  hidden: boolean;
}

export interface SeedFact {
  subject: string;
  key: string;
  value: string;
  text: string;
}

export interface StartSeed {
  place: string;
  present: string[];
  situation: string;
  when: string;
  /** The hour of Day 1 when play starts, 0 to 23. Missing or null means 8. */
  hour?: number | null;
}

export type OpenMode = "delegated" | "discover" | "to_agree";

export interface OpenDetail {
  text: string;
  mode: OpenMode;
}

export interface AdventureSeed {
  premise: string;
  pc: PcSeed;
  entities: SeedEntity[];
  boundaries: string[];
  decided: SeedFact[];
  proposed: SeedFact[];
  story_language: string;
  themes: string[];
  gm_agreement: string;
  resolution: string;
  min_age: number | null;
  start: StartSeed;
  open_details: OpenDetail[];
  /** The Detail Level the Campaign starts with. `excluded` by default. */
  detail: DetailLevel;
}

/** How an Intimate Scene is told, if at all (glossary: Detail Level). */
export type DetailLevel = "excluded" | "implied" | "explicit";

/** A fact the PC knows. `open`: planned or pending, not done. */
export interface KnownFact {
  subject: string;
  key: string;
  value: string;
  text: string;
  open: boolean;
}

export type GapKind =
  | "missing"
  | "age_missing"
  | "age_unconfirmed"
  | "age_below_min"
  | "unknown_name"
  | "duplicate_name"
  | "pc_in_roster"
  | "wrong_kind"
  | "seed_block_too_long"
  | "slot_conflict";

/**
 * One thing to fix before approval. `at` locates the field (see `Gap` in
 * Rust). `with`: the other entry of a pair, in the same path form, on a
 * `duplicate_name` or a `slot_conflict` gap only; absent on older data.
 */
export interface Gap {
  kind: GapKind;
  at: string;
  with?: string;
}

export type DraftState = "draft" | "approved" | "built";

/** A stage of a draft or a World Build, as the shell names it. */
export type BuildStage =
  | "draft_roster"
  | "draft_seed"
  | "draft_knowledge"
  | "hidden_facts"
  | "npc_state"
  | "world_events"
  | "start_state"
  | "commit";

/** Why a build stopped: names only, never model text. */
export interface BuildFailure {
  stage: string;
  check: string;
}

/** Where a character's current age comes from. The core derives it. */
export type AgeSource = "from_text" | "proposed" | "edited";

/**
 * One roster entry as a review command names it: its row (`null` for the
 * PC, else the index in `seed.entities`), with its exact main name and kind
 * as a check. The core refuses an entry that its row does not hold.
 */
export interface EntryRef {
  index: number | null;
  name: string;
  kind: EntityKind;
}

/** One character's age as the core sees it: the PC first, then the roster order. */
export interface AgeLine {
  /** The main name, as the seed writes it. */
  name: string;
  /** The row: `null` for the PC, else the index in `seed.entities`. */
  index: number | null;
  kind: EntityKind;
  source: AgeSource;
  /** The Player confirmed this very age, for this very entry, through `confirm_age`. */
  confirmed: boolean;
}

/** A list the draft run cut at its cap. `at` has the path form of `Gap.at`. */
export interface Cut {
  at: string;
  cap: number;
}

export interface DraftView {
  draft_id: string;
  state: DraftState;
  seed: AdventureSeed;
  knowledge: KnownFact[];
  ages: AgeLine[];
  cut: Cut[];
  gaps: Gap[];
  last_failure: BuildFailure | null;
}

export interface DraftLine {
  draft_id: string;
  title: string;
  state: DraftState;
}

/**
 * The `build_progress` event. `stage` stays a string: a newer shell may add
 * one. `attempt` of `attempts` counts the tries of the running model call;
 * `retry_after_timeout` is true when the call before it timed out.
 */
export interface BuildProgress {
  stage: string;
  done: number;
  total: number;
  attempt: number;
  attempts: number;
  retry_after_timeout: boolean;
}

export type Approval = { ok: true; draft: DraftView } | { ok: false; gaps: Gap[] };

/** The op_id of the Opening Turn's narration chunks. */
export const BEGIN_OP_ID = "__begin__";

/** The start of the op_id of a renarration's chunks; the shell's `RENARRATE_OP_PREFIX`. */
export const RENARRATE_OP_PREFIX = "__renarrate__";

/** The op_id of the chunks of `renarrate(turnId)`: not the Turn's own op_id. */
export function renarrateOpId(turnId: number): string {
  return `${RENARRATE_OP_PREFIX}${turnId}`;
}

export interface NarrationChunk {
  op_id: string;
  text: string;
}

export interface TurnCommitted {
  turn_id: number;
  roll: RollResult | null;
}

export interface TurnFailed {
  op_id: string;
  reason: string;
}

/** Why the core refused a Turn: the Detail Level, the age rule, or nobody goes along. */
export type RefusalKind = "excluded" | "adults_only" | "unwilling";

/**
 * The `turn_refused` event: a Refusal, not a failed Turn. `to_confirm` holds
 * the characters the Player may confirm as adults from it, by name and with
 * no age (an `adults_only` Refusal only).
 */
export interface TurnRefused {
  op_id: string;
  text: string;
  kind: RefusalKind;
  to_confirm: NameRef[];
}

/** One attempt at a Turn: its op_id, known before the result arrives. */
export interface Attempt {
  opId: string;
  result: Promise<TurnSummary>;
}

export interface PendingTurn {
  opId: string;
  cmd: "submit_intent" | "correct_last" | "end_scene";
  args: Record<string, unknown>;
}

export interface TurnClient {
  /** Plays a new Intent under a fresh op_id. */
  submit(text: string): Attempt;
  /** Corrects the last Turn under a fresh op_id. */
  correct(clarified: string, why: string): Attempt;
  /** "End scene": the Closing Turn of the open Intimate Scene, under a fresh op_id. */
  endScene(): Attempt;
  /** Sends the failed Turn again under the same op_id; null if none failed. */
  retry(): Attempt | null;
  /** The Turn that failed and can be retried, if any. */
  pending(): PendingTurn | null;
  /** Forgets the pending Turn. Call it when another campaign opens. */
  reset(): void;
}

/**
 * Builds Turn calls. Each new Intent or Correction gets a fresh op_id. A retry
 * reuses the op_id of the failed call, so the core can never commit the same
 * Turn twice.
 */
export function createTurnClient(
  invoke: Invoke,
  newOpId: () => string = () => crypto.randomUUID(),
): TurnClient {
  let pending: PendingTurn | null = null;

  const send = (turn: PendingTurn): Attempt => {
    pending = turn;
    const result = invoke<TurnSummary>(turn.cmd, turn.args).then(
      (summary) => {
        if (pending === turn) pending = null;
        return summary;
      },
      (e: unknown) => {
        // A Refusal is not a failed Turn: nothing to send again.
        if (refusalFromError(e) !== null && pending === turn) pending = null;
        throw e;
      },
    );
    return { opId: turn.opId, result };
  };

  const fresh = (cmd: PendingTurn["cmd"], rest: Record<string, unknown>): Attempt => {
    const opId = newOpId();
    return send({ opId, cmd, args: { op_id: opId, ...rest } });
  };

  return {
    submit: (text) => fresh("submit_intent", { text }),
    correct: (clarified, why) => fresh("correct_last", { clarified, why }),
    endScene: () => fresh("end_scene", {}),
    retry: () => (pending ? send(pending) : null),
    pending: () => pending,
    reset: () => {
      pending = null;
    },
  };
}

/** The other commands, over any `invoke`. */
export function createApi(invoke: Invoke) {
  return {
    defaults: () => invoke<Defaults>("defaults"),
    /** The Play screen, from PC Knowledge only. Call it after a Turn ends, never during one. */
    getPlayView: () => invoke<PlayView>("get_play_view"),
    modelStatus: () => invoke<string>("model_status"),
    /** The Content Packs the app loaded at start, and the folders it refused. */
    listPacks: () => invoke<PackLine[]>("list_packs"),
    /** The home cards, the latest played first. */
    listCampaigns: () => invoke<CampaignCard[]>("list_campaigns"),
    /** "Remove from list": moves the card's file to the archive folder of the data dir. Nothing is deleted. */
    removeCampaign: (cardId: string) => invoke<null>("remove_campaign", { card_id: cardId }),
    openCampaign: (dbPath: string) => invoke<CampaignSummary>("open_campaign", { db_path: dbPath }),
    askOoc: (text: string) => invoke<string>("ask_ooc", { text }),
    renarrate: (turnId: number) => invoke<string>("renarrate", { turn_id: turnId }),
    /** "Fade to black": sets the fade flag with no lock; see `FadeResult`. */
    fadeToBlack: (opId: string) => invoke<FadeResult>("fade_to_black", { op_id: opId }),
    /**
     * "Confirm" in play: the core confirms that one character is an adult,
     * in an OOC Turn, and returns the new Play view. It refuses, with one
     * fixed sentence, a character it cannot confirm.
     */
    confirmAgeInPlay: (entityId: number, opId: string) =>
      invoke<PlayView>("confirm_age_in_play", { entity_id: entityId, op_id: opId }),
    /** The Detail Level in play, in an OOC Turn; returns the new Play view. */
    setDetailLevel: (level: DetailLevel, opId: string) =>
      invoke<PlayView>("set_detail_level", { level, op_id: opId }),
    getCharacter: () => invoke<CharacterView>("get_character"),
    /** One card. Refused with one fixed text for an entity the PC does not know. */
    getEntityCard: (entityId: number) =>
      invoke<EntityCard>("get_entity_card", { entity_id: entityId }),
    getJournal: () => invoke<JournalView>("get_journal"),
    /** A new Note, free or about one target. It never reaches the GM. */
    addNote: (body: string, about: Target | null) => invoke<Note>("add_note", { body, about }),
    editNote: (noteId: number, body: string) => invoke<Note>("edit_note", { note_id: noteId, body }),
    /** Removes the Note and its Pin. */
    deleteNote: (noteId: number) => invoke<null>("delete_note", { note_id: noteId }),
    setPin: (target: Target, on: boolean) => invoke<null>("set_pin", { target, on }),
    setDebug: (on: boolean) => invoke<boolean>("set_debug", { on }),
    getDebug: (turnId: number) => invoke<DebugView>("get_debug", { turn_id: turnId }),
    draftFromText: (text: string) => invoke<DraftView>("draft_from_text", { text }),
    listDrafts: () => invoke<DraftLine[]>("list_drafts"),
    getDraft: (draftId: string) => invoke<DraftView>("get_draft", { draft_id: draftId }),
    /** Saves an edit. It never confirms an age, and never renames: see the next two. */
    saveDraft: (draftId: string, seed: AdventureSeed, knowledge: KnownFact[]) =>
      invoke<DraftView>("save_draft", { draft_id: draftId, seed, knowledge }),
    /** The core confirms the current age of one character entry. */
    confirmAge: (draftId: string, entry: EntryRef) =>
      invoke<DraftView>("confirm_age", { draft_id: draftId, entry }),
    /** Renames one entry in every field of the draft that names it. */
    renameDraftEntity: (draftId: string, entry: EntryRef, to: string) =>
      invoke<DraftView>("rename_draft_entity", { draft_id: draftId, entry, to }),
    /** Approves the draft, or returns its gaps. Any other error rejects. */
    approveDraft: async (draftId: string): Promise<Approval> => {
      try {
        return { ok: true, draft: await invoke<DraftView>("approve_draft", { draft_id: draftId }) };
      } catch (e) {
        const gaps = gapsFromError(e);
        if (gaps) return { ok: false, gaps };
        throw e;
      }
    },
    /** Builds the world; resolves with the opened Campaign. */
    startBuild: (draftId: string) => invoke<CampaignSummary>("start_build", { draft_id: draftId }),
    /** Asks the running draft or build to stop. It may still finish. */
    cancelBuild: () => invoke<null>("cancel_build"),
    begin: () => invoke<TurnSummary>("begin"),
    deleteDraft: (draftId: string) => invoke<null>("delete_draft", { draft_id: draftId }),
    /** Reveals the hidden world: debug mode only. */
    getBuildDebug: (draftId: string) =>
      invoke<DebugLine[]>("get_build_debug", { draft_id: draftId }),
  };
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null;

/**
 * The sentence of a Refusal, or null for any other error. The shell sends a
 * Refusal as `{refusal: <sentence>}`: the core did not play the Turn, and
 * it did not fail (review M3).
 */
export function refusalFromError(e: unknown): string | null {
  return isRecord(e) && typeof e.refusal === "string" ? e.refusal : null;
}

/**
 * The characters a Refusal offers to confirm, each with a name the PC can
 * use; empty for a Refusal with no list and for any other error. The shell
 * sends them with the sentence: `{refusal, kind, to_confirm}` (Task 8).
 */
export function toConfirmFromError(e: unknown): NameRef[] {
  if (refusalFromError(e) === null || !isRecord(e) || !Array.isArray(e.to_confirm)) return [];
  const names: NameRef[] = [];
  for (const item of e.to_confirm as unknown[]) {
    if (
      !isRecord(item) ||
      typeof item.entity_id !== "number" ||
      typeof item.kind !== "string" ||
      typeof item.name !== "string"
    ) {
      return [];
    }
    names.push({ entity_id: item.entity_id, kind: item.kind as EntityKind, name: item.name });
  }
  return names;
}

/** The gaps of an `approve_draft` error, or null for any other error. */
export function gapsFromError(e: unknown): Gap[] | null {
  if (!Array.isArray(e)) return null;
  const gaps: Gap[] = [];
  for (const item of e as unknown[]) {
    if (!isRecord(item) || typeof item.kind !== "string" || typeof item.at !== "string") {
      return null;
    }
    const gap: Gap = { kind: item.kind as GapKind, at: item.at };
    if (typeof item.with === "string") gap.with = item.with;
    gaps.push(gap);
  }
  return gaps;
}

/** A `build_progress` payload, or null when it has not that shape. */
export function parseBuildProgress(payload: unknown): BuildProgress | null {
  if (!isRecord(payload)) return null;
  const { stage, done, total, attempt, attempts, retry_after_timeout } = payload;
  if (typeof stage !== "string" || typeof done !== "number" || typeof total !== "number") {
    return null;
  }
  const count = (n: unknown) => (typeof n === "number" && n >= 1 ? n : 1);
  return {
    stage,
    done,
    total,
    attempt: count(attempt),
    attempts: count(attempts),
    retry_after_timeout: retry_after_timeout === true,
  };
}

export type Api = ReturnType<typeof createApi>;

/**
 * The notice to show with a committed Turn. A Turn whose narration failed is
 * committed all the same: the view shows its Narration as missing and offers
 * to narrate it again.
 */
export function turnNotice(turn: TurnSummary): string | null {
  return turn.narration === null
    ? `Committed, narration interrupted: ${turn.narration_error ?? "unknown error"}`
    : null;
}

/** The text of a command error: Tauri rejects with the serialized AppError. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  const refusal = refusalFromError(e);
  if (refusal !== null) return refusal;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** "Day 1, 08:05" from World Clock minutes. */
export function clockText(minutes: number): string {
  const day = Math.floor(minutes / 1440) + 1;
  const hh = String(Math.floor((minutes % 1440) / 60)).padStart(2, "0");
  const mm = String(minutes % 60).padStart(2, "0");
  return `Day ${day}, ${hh}:${mm}`;
}

export function rollText(roll: RollResult): string {
  return `Roll ${roll.value} vs ${roll.difficulty}: ${roll.success ? "success" : "failure"}`;
}
