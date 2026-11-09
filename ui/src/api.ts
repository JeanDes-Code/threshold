// Typed access to the Tauri commands and events of the shell (src-tauri).
// Field names mirror the Rust types: snake_case.

export type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export interface RollResult {
  seed: number;
  difficulty: number;
  value: number;
  success: boolean;
}

export interface TurnLine {
  turn_id: number;
  kind: string;
  intent: string;
  narration: string | null;
  roll: RollResult | null;
  /** A later Correction reverted this Turn. */
  reverted: boolean;
}

export interface CampaignSummary {
  db_path: string;
  pc_name: string;
  premise: string;
  turns: TurnLine[];
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
  facts: string[];
}

export interface JournalEntry {
  subject: string;
  text: string;
  source: string | null;
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

export interface Defaults {
  seed_path: string;
  world_path: string;
  data_dir: string;
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
    summary: () => invoke<CampaignSummary>("campaign_summary"),
    modelStatus: () => invoke<string>("model_status"),
    listCampaigns: () => invoke<string[]>("list_campaigns"),
    newCampaign: (seedPath: string, worldPath: string) =>
      invoke<CampaignSummary>("new_campaign", { seed_path: seedPath, world_path: worldPath }),
    openCampaign: (dbPath: string) => invoke<CampaignSummary>("open_campaign", { db_path: dbPath }),
    askOoc: (text: string) => invoke<string>("ask_ooc", { text }),
    renarrate: (turnId: number) => invoke<string>("renarrate", { turn_id: turnId }),
    getCharacter: () => invoke<CharacterView>("get_character"),
    getJournal: () => invoke<JournalView>("get_journal"),
    setDebug: (on: boolean) => invoke<boolean>("set_debug", { on }),
    getDebug: (turnId: number) => invoke<DebugView>("get_debug", { turn_id: turnId }),
  };
}

export type Api = ReturnType<typeof createApi>;

/**
 * The Turn line of a committed Turn, and the notice to show with it. A Turn
 * whose narration failed is committed all the same: its line has no
 * narration, so the view offers to narrate it again.
 */
export function committedLine(
  turn: TurnSummary,
  kind: string,
  intent: string,
): { line: TurnLine; notice: string | null } {
  return {
    line: {
      turn_id: turn.turn_id,
      kind,
      intent,
      narration: turn.narration,
      roll: turn.roll,
      reverted: false,
    },
    notice:
      turn.narration === null
        ? `Committed, narration interrupted: ${turn.narration_error ?? "unknown error"}`
        : null,
  };
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
