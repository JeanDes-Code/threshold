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

/** A committed Turn. `narration` is null when the narration failed after the Commit. */
export interface TurnSummary {
  turn_id: number;
  roll: RollResult | null;
  narration: string | null;
  narration_error: string | null;
  total_ms: number;
}

export interface CharacterView {
  name: string;
  concept: string;
  clock_min: number;
  facts: CharacterFact[];
}

/** One thing the PC knows about itself. */
export interface CharacterFact {
  text: string;
  /** An open item: planned or pending, not done. */
  open: boolean;
}

export interface JournalEntry {
  subject: string;
  text: string;
  source: string | null;
  /** An open item: planned or pending, not done. */
  open: boolean;
}

export interface JournalView {
  observations: JournalEntry[];
  claims: JournalEntry[];
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

export type TurnKind = "setup" | "play" | "correction" | "ooc" | "opening";

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
  | { kind: "speech"; speaker: NameRef; parts: Part[] };

export type TurnBody = { kind: "narration"; blocks: Block[] } | { kind: "missing" };

export interface StoryTurn {
  turn_id: number;
  kind: TurnKind;
  intent: string;
  roll: RollResult | null;
  /** A later Correction reverted this Turn. Its body holds prose blocks only. */
  reverted: boolean;
  body: TurnBody;
}

/** A PC Belief about the PC on a state key (never `location`). */
export interface StateLine {
  key: string;
  value: string;
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

/** The mature content module adds a variant; the UI reads the tag only. */
export type SceneState = { kind: "plain" };

export interface PlayView {
  pc_name: string;
  place: PlaceLine | null;
  clock_min: number;
  needs_begin: boolean;
  can_correct: boolean;
  scene: SceneState;
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
}

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

/** One thing to fix before approval. `at` locates the field (see `Gap` in Rust). */
export interface Gap {
  kind: GapKind;
  at: string;
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

export interface DraftView {
  draft_id: string;
  state: DraftState;
  seed: AdventureSeed;
  knowledge: KnownFact[];
  ages_to_confirm: string[];
  /** Information only: the names whose age the model says the text states. */
  ages_stated: string[];
  gaps: Gap[];
  last_failure: BuildFailure | null;
}

export interface DraftLine {
  draft_id: string;
  title: string;
  state: DraftState;
}

/** The `build_progress` event. `stage` stays a string: a newer shell may add one. */
export interface BuildProgress {
  stage: string;
  done: number;
  total: number;
}

export type Approval = { ok: true; draft: DraftView } | { ok: false; gaps: Gap[] };

/** The op_id of the Opening Turn's narration chunks. */
export const BEGIN_OP_ID = "__begin__";

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

/** One attempt at a Turn: its op_id, known before the result arrives. */
export interface Attempt {
  opId: string;
  result: Promise<TurnSummary>;
}

export interface PendingTurn {
  opId: string;
  cmd: "submit_intent" | "correct_last";
  args: Record<string, unknown>;
}

export interface TurnClient {
  /** Plays a new Intent under a fresh op_id. */
  submit(text: string): Attempt;
  /** Corrects the last Turn under a fresh op_id. */
  correct(clarified: string, why: string): Attempt;
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
    const result = invoke<TurnSummary>(turn.cmd, turn.args).then((summary) => {
      if (pending === turn) pending = null;
      return summary;
    });
    return { opId: turn.opId, result };
  };

  const fresh = (cmd: PendingTurn["cmd"], rest: Record<string, unknown>): Attempt => {
    const opId = newOpId();
    return send({ opId, cmd, args: { op_id: opId, ...rest } });
  };

  return {
    submit: (text) => fresh("submit_intent", { text }),
    correct: (clarified, why) => fresh("correct_last", { clarified, why }),
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
    listCampaigns: () => invoke<string[]>("list_campaigns"),
    openCampaign: (dbPath: string) => invoke<CampaignSummary>("open_campaign", { db_path: dbPath }),
    askOoc: (text: string) => invoke<string>("ask_ooc", { text }),
    renarrate: (turnId: number) => invoke<string>("renarrate", { turn_id: turnId }),
    getCharacter: () => invoke<CharacterView>("get_character"),
    getJournal: () => invoke<JournalView>("get_journal"),
    setDebug: (on: boolean) => invoke<boolean>("set_debug", { on }),
    getDebug: (turnId: number) => invoke<DebugView>("get_debug", { turn_id: turnId }),
    draftFromText: (text: string) => invoke<DraftView>("draft_from_text", { text }),
    listDrafts: () => invoke<DraftLine[]>("list_drafts"),
    getDraft: (draftId: string) => invoke<DraftView>("get_draft", { draft_id: draftId }),
    saveDraft: (
      draftId: string,
      seed: AdventureSeed,
      knowledge: KnownFact[],
      agesToConfirm: string[],
      agesStated: string[],
    ) =>
      invoke<DraftView>("save_draft", {
        draft_id: draftId,
        seed,
        knowledge,
        ages_to_confirm: agesToConfirm,
        ages_stated: agesStated,
      }),
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

/** The gaps of an `approve_draft` error, or null for any other error. */
export function gapsFromError(e: unknown): Gap[] | null {
  if (!Array.isArray(e)) return null;
  const gaps: Gap[] = [];
  for (const item of e as unknown[]) {
    if (!isRecord(item) || typeof item.kind !== "string" || typeof item.at !== "string") {
      return null;
    }
    gaps.push({ kind: item.kind as GapKind, at: item.at });
  }
  return gaps;
}

/** A `build_progress` payload, or null when it has not that shape. */
export function parseBuildProgress(payload: unknown): BuildProgress | null {
  if (!isRecord(payload)) return null;
  const { stage, done, total } = payload;
  if (typeof stage !== "string" || typeof done !== "number" || typeof total !== "number") {
    return null;
  }
  return { stage, done, total };
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
