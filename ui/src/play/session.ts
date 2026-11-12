// When the Play screen reads the core, and the state of the running Turn.
//
// The UI calls `get_play_view` once after a Campaign opens, once after each
// Turn ends (committed or failed), and once after each note command. A
// running Turn holds the session lock, so no view call runs inside one.
//
// The Turn state lives above the tabs: App holds one store per app, and a tab
// switch does not touch it. Only another Campaign resets it.

import {
  errorText,
  refusalFromError,
  toConfirmFromError,
  turnNotice,
  type Api,
  type Attempt,
  type NameRef,
  type NarrationChunk,
  type PlayView,
} from "../api";

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
  /**
   * The sentence of the last Refusal: the core did not play the Turn, and
   * it did not fail. No retry; the Intent goes back to the field. The next
   * call clears it.
   */
  refusal: string | null;
  /**
   * The characters the last Refusal offers to confirm as adults, by name
   * (Task 8). A confirmation takes one out; the next call clears it.
   */
  toConfirm: NameRef[];
  /** The failed Turn can be sent again under its op_id. */
  canRetry: boolean;
  /** A Turn that committed with a problem, or a command that failed. */
  notice: string | null;
  /** The last OOC answer. */
  answer: string | null;
  /**
   * The Player faded the running Narration, or a fade with no Turn running
   * is in flight: the stream text is off the screen, and later chunks of
   * that op_id are dropped. The end of the call clears it.
   */
  faded: boolean;
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
  refusal: null,
  toConfirm: [],
  canRetry: false,
  notice: null,
  answer: null,
  faded: false,
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
  /**
   * Narrates a committed Turn again. Its chunks come under `opId` only. A
   * fade while it streams works as for a Turn: when it ends, the fade is
   * sent once more with `opId`.
   */
  renarrate(opId: string, call: () => Promise<unknown>): Promise<void>;
  /**
   * "Fade to black". While a Turn runs, the stream text leaves the screen at
   * once, later chunks of its op_id are dropped, and `call` asks the shell
   * to fade it, with that op_id: the Turn's own end reloads the view. When
   * the Turn ends read in full (it ended before it read the flag), `call`
   * runs once more with its op_id, before the reload, and the shell fades
   * it then. With no Turn running, `call` gets null and is the whole fade
   * (a Closing Turn born faded), then the view reloads. A second fade before
   * the first ends changes nothing.
   */
  fade(call: (opId: string | null) => Promise<unknown>): Promise<void>;
  /** Asks the GM out of character. Refused (false) while another call runs. */
  ask(call: () => Promise<string>): Promise<boolean>;
  chunk(chunk: NarrationChunk): void;
  notify(text: string | null): void;
  dismissFailure(): void;
  dismissAnswer(): void;
  /** The core confirmed this character: the Refusal no longer offers it. */
  confirmed(entityId: number): void;
  /** Forgets everything, a running call included. Call it when another Campaign opens. */
  reset(): void;
}

export function createTurnStore(deps: TurnDeps): TurnStore {
  let state = IDLE_TURN;
  // A reset starts a new generation; a late result of an older one is dropped.
  let generation = 0;
  // The op_ids the Player faded: their chunks never reach the screen again.
  const fadedOps = new Set<string>();
  // The call of the last fade, for a Turn that ended before it read the flag.
  let fadeCall: ((opId: string | null) => Promise<unknown>) | null = null;
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
        refusal: null,
        toConfirm: [],
        canRetry: false,
        notice: null,
        startedMs: now(),
      });
      let committed = false;
      let notice: string | null = null;
      let failure: string | null = null;
      let refusal: string | null = null;
      let toConfirm: NameRef[] = [];
      let read = false;
      try {
        const summary = await made.result;
        notice = turnNotice(summary);
        committed = true;
        read = !summary.faded;
      } catch (e) {
        refusal = refusalFromError(e);
        toConfirm = toConfirmFromError(e);
        if (refusal === null) failure = errorText(e);
      }
      if (mine !== generation) return committed;
      // The Player faded this Turn, and it ended read in full: fade it now.
      const late = fadeCall;
      if (read && fadedOps.has(made.opId) && late !== null) {
        try {
          await late(made.opId);
        } catch (e) {
          notice = `Fade to black failed: ${errorText(e)}`;
        }
        if (mine !== generation) return committed;
      }
      set({ opId: null });
      // The new view lands before the stream goes, so the story does not jump back.
      const reloadNotice = await reload();
      if (mine !== generation) return committed;
      set({
        busy: false,
        stream: null,
        pendingIntent: null,
        failure,
        refusal,
        toConfirm,
        canRetry: failure !== null && deps.canRetry(),
        notice: reloadNotice ?? notice,
        faded: false,
        startedMs: null,
      });
      return committed;
    },

    renarrate: async (opId, call) => {
      if (state.busy) return;
      const mine = generation;
      // A renarration of the same Turn streams again, even after a fade of
      // an earlier one.
      fadedOps.delete(opId);
      set({
        busy: true,
        opId,
        stream: "",
        failure: null,
        refusal: null,
        toConfirm: [],
        canRetry: false,
        notice: null,
        startedMs: now(),
      });
      let notice: string | null = null;
      let told = false;
      try {
        await call();
        told = true;
      } catch (e) {
        // The Turn stays committed; only its narration failed again.
        notice = `Committed, narration interrupted: ${errorText(e)}`;
      }
      if (mine !== generation) return;
      // The Player faded this renarration, and it may have ended before it
      // read the flag: send the fade again with its op_id. The shell fades
      // the Turn it told when that Turn is the latest; a fade it already
      // took changes nothing.
      const late = fadeCall;
      if (told && fadedOps.has(opId) && late !== null) {
        try {
          await late(opId);
        } catch (e) {
          notice = `Fade to black failed: ${errorText(e)}`;
        }
        if (mine !== generation) return;
      }
      set({ opId: null });
      const reloadNotice = await reload();
      if (mine !== generation) return;
      set({ busy: false, stream: null, notice: reloadNotice ?? notice, faded: false, startedMs: null });
    },

    fade: async (call) => {
      if (state.faded) return;
      fadeCall = call;
      if (state.busy) {
        const opId = state.opId;
        if (opId !== null) fadedOps.add(opId);
        set({ faded: true, stream: state.stream === null ? null : "" });
        try {
          await call(opId);
        } catch (e) {
          set({ notice: `Fade to black failed: ${errorText(e)}` });
        }
        return;
      }
      const mine = generation;
      set({ busy: true, faded: true, failure: null, refusal: null, toConfirm: [], notice: null, startedMs: now() });
      let notice: string | null = null;
      try {
        await call(null);
      } catch (e) {
        notice = `Fade to black failed: ${errorText(e)}`;
      }
      if (mine !== generation) return;
      const reloadNotice = await reload();
      if (mine !== generation) return;
      set({ busy: false, faded: false, notice: reloadNotice ?? notice, startedMs: null });
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
      if (state.opId === null || chunk.op_id !== state.opId || fadedOps.has(chunk.op_id)) return;
      set({ stream: (state.stream ?? "") + chunk.text });
    },

    notify: (text) => set({ notice: text }),

    dismissFailure: () => {
      deps.forget();
      set({ failure: null, canRetry: false });
    },

    dismissAnswer: () => set({ answer: null }),

    confirmed: (entityId) => set({ toConfirm: state.toConfirm.filter((who) => who.entity_id !== entityId) }),

    reset: () => {
      generation += 1;
      state = IDLE_TURN;
      for (const listener of listeners) listener();
    },
  };
}
