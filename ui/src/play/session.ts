// When the Play screen reads the core, and the state of the running Turn.
//
// The UI calls `get_play_view` once after a Campaign opens, once after each
// Turn ends (committed or failed), and once after each note command. A
// running Turn holds the session lock, so no view call runs inside one.
//
// The Turn state lives above the tabs: App holds one store per app, and a tab
// switch does not touch it. Only another Campaign resets it.

import { errorText, turnNotice, type Api, type Attempt, type NarrationChunk, type PlayView } from "../api";

export interface PlaySession {
  opened(): Promise<PlayView>;
  turnEnded(): Promise<PlayView>;
  noteChanged(): Promise<PlayView>;
}

export function createPlaySession(api: Pick<Api, "getPlayView">): PlaySession {
  const load = () => api.getPlayView();
  return { opened: load, turnEnded: load, noteChanged: load };
}

/** The running Turn, a renarration or an OOC question, and what the last one left. */
export interface TurnState {
  /** True from the call until its result lands and the view has reloaded. */
  busy: boolean;
  /** The op_id whose narration chunks the story shows; null when it takes none. */
  opId: string | null;
  /** The raw Narration of the running call: "" before its first chunk; null when nothing streams. */
  stream: string | null;
  /** The Intent of the running Turn. */
  pendingIntent: string | null;
  /** Why the last Turn failed. It stays until the Player retries or dismisses it. */
  failure: string | null;
  /** The failed Turn can be sent again under its op_id. */
  canRetry: boolean;
  /** A Turn that committed with a problem, or a command that failed. */
  notice: string | null;
  /** The last OOC answer. */
  answer: string | null;
  /**
   * When the running call started, in ms since the epoch; null while idle.
   * The phase line counts its seconds from here, so a tab switch, which
   * mounts the Play screen again, does not start them over.
   */
  startedMs: number | null;
}

export const IDLE_TURN: TurnState = {
  busy: false,
  opId: null,
  stream: null,
  pendingIntent: null,
  failure: null,
  canRetry: false,
  notice: null,
  answer: null,
  startedMs: null,
};

export interface TurnDeps {
  /** The one view call after a Turn ends. The caller keeps the view. */
  reload(): Promise<void>;
  /** The turn client still holds the failed Turn. */
  canRetry(): boolean;
  /** The turn client forgets the failed Turn. */
  forget(): void;
  /** The clock, in ms since the epoch; `Date.now` when left out. */
  now?: () => number;
}

export interface TurnStore {
  get(): TurnState;
  subscribe(listener: () => void): () => void;
  /**
   * Runs one Turn. Refused while another call runs: `attempt` is then not
   * called, so nothing is sent. Resolves true when the Turn committed.
   */
  run(attempt: () => Attempt | null, intent: string | null): Promise<boolean>;
  /** Narrates a committed Turn again. Its chunks come under `opId` only. */
  renarrate(opId: string, call: () => Promise<unknown>): Promise<void>;
  /** Asks the GM out of character. Refused (false) while another call runs. */
  ask(call: () => Promise<string>): Promise<boolean>;
  chunk(chunk: NarrationChunk): void;
  notify(text: string | null): void;
  dismissFailure(): void;
  dismissAnswer(): void;
  /** Forgets everything, a running call included. Call it when another Campaign opens. */
  reset(): void;
}

export function createTurnStore(deps: TurnDeps): TurnStore {
  let state = IDLE_TURN;
  // A reset starts a new generation; a late result of an older one is dropped.
  let generation = 0;
  const listeners = new Set<() => void>();
  const now = deps.now ?? Date.now;

  const set = (change: Partial<TurnState>) => {
    state = { ...state, ...change };
    for (const listener of listeners) listener();
  };

  /** The reload; resolves with a notice when it failed. */
  const reload = async (): Promise<string | null> => {
    try {
      await deps.reload();
      return null;
    } catch (e) {
      return `Could not reload the story: ${errorText(e)}`;
    }
  };

  return {
    get: () => state,

    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },

    run: async (attempt, intent) => {
      if (state.busy) return false;
      const made = attempt();
      if (made === null) return false;
      const mine = generation;
      set({
        busy: true,
        opId: made.opId,
        stream: "",
        pendingIntent: intent,
        failure: null,
        canRetry: false,
        notice: null,
        startedMs: now(),
      });
      let committed = false;
      let notice: string | null = null;
      let failure: string | null = null;
      try {
        notice = turnNotice(await made.result);
        committed = true;
      } catch (e) {
        failure = errorText(e);
      }
      if (mine !== generation) return committed;
      set({ opId: null });
      // The new view lands before the stream goes, so the story does not jump back.
      const reloadNotice = await reload();
      if (mine !== generation) return committed;
      set({
        busy: false,
        stream: null,
        pendingIntent: null,
        failure,
        canRetry: failure !== null && deps.canRetry(),
        notice: reloadNotice ?? notice,
        startedMs: null,
      });
      return committed;
    },

    renarrate: async (opId, call) => {
      if (state.busy) return;
      const mine = generation;
      set({ busy: true, opId, stream: "", failure: null, canRetry: false, notice: null, startedMs: now() });
      let notice: string | null = null;
      try {
        await call();
      } catch (e) {
        // The Turn stays committed; only its narration failed again.
        notice = `Committed, narration interrupted: ${errorText(e)}`;
      }
      if (mine !== generation) return;
      set({ opId: null });
      const reloadNotice = await reload();
      if (mine !== generation) return;
      set({ busy: false, stream: null, notice: reloadNotice ?? notice, startedMs: null });
    },

    ask: async (call) => {
      if (state.busy) return false;
      const mine = generation;
      set({ busy: true, startedMs: now() });
      let answer: string;
      let ok = false;
      try {
        answer = await call();
        ok = true;
      } catch (e) {
        answer = `OOC failed: ${errorText(e)}`;
      }
      if (mine !== generation) return ok;
      set({ busy: false, answer, startedMs: null });
      return ok;
    },

    chunk: (chunk) => {
      if (state.opId === null || chunk.op_id !== state.opId) return;
      set({ stream: (state.stream ?? "") + chunk.text });
    },

    notify: (text) => set({ notice: text }),

    dismissFailure: () => {
      deps.forget();
      set({ failure: null, canRetry: false });
    },

    dismissAnswer: () => set({ answer: null }),

    reset: () => {
      generation += 1;
      state = IDLE_TURN;
      for (const listener of listeners) listener();
    },
  };
}
