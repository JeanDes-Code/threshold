import { describe, expect, it } from "vitest";
import { renarrateOpId, type Attempt, type TurnSummary } from "../api";
import { IDLE_TURN, createTurnStore, type TurnDeps } from "./session";

const committed: TurnSummary = { turn_id: 4, roll: null, narration: "ok", narration_error: null, total_ms: 1 };

/** A promise the test settles by hand. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Lets every settled promise run its callbacks. */
const settle = () => new Promise((done) => setTimeout(done, 0));

function harness(retryable = true, now = () => 1_000) {
  const log = { reloads: 0, forgets: 0 };
  const deps: TurnDeps = {
    now,
    reload: () => {
      log.reloads += 1;
      return Promise.resolve();
    },
    canRetry: () => retryable,
    forget: () => {
      log.forgets += 1;
    },
  };
  return { store: createTurnStore(deps), log };
}

function attempt(opId: string) {
  const result = deferred<TurnSummary>();
  const made: Attempt = { opId, result: result.promise };
  return { result, make: () => made };
}

// The store holds the Turn state above the tabs: no screen needs to be
// mounted for a chunk, a result or a failure to land.
describe("createTurnStore", () => {
  it("a running call holds the time it started, and an idle store holds none", async () => {
    let clock = 5_000;
    const { store } = harness(true, () => clock);
    expect(store.get().startedMs).toBeNull();
    const turn = attempt("op-1");
    const done = store.run(turn.make, "I knock.");
    expect(store.get().startedMs).toBe(5_000);
    clock = 9_000;
    // A screen that mounts again (a tab switch) reads the same start.
    expect(store.get().startedMs).toBe(5_000);
    turn.result.resolve(committed);
    await done;
    expect(store.get().startedMs).toBeNull();
  });

  it("a chunk for the running op appends while no Play screen listens", () => {
    const { store } = harness();
    const turn = attempt("op-1");
    void store.run(turn.make, "I knock.");
    store.chunk({ op_id: "op-1", text: "Orsel: " });
    store.chunk({ op_id: "op-1", text: '"Late."' });
    expect(store.get().stream).toBe('Orsel: "Late."');
    expect(store.get().pendingIntent).toBe("I knock.");
  });

  it("a chunk for another op is ignored, and so is any chunk when no Turn runs", () => {
    const { store } = harness();
    store.chunk({ op_id: "op-1", text: "early" });
    expect(store.get()).toEqual(IDLE_TURN);
    const turn = attempt("op-1");
    void store.run(turn.make, "I knock.");
    store.chunk({ op_id: "op-0", text: "stale" });
    store.chunk({ op_id: "op-1", text: "fresh" });
    expect(store.get().stream).toBe("fresh");
  });

  it("busy stays true until the result arrives, then the view reloads once", async () => {
    const { store, log } = harness();
    const turn = attempt("op-1");
    const done = store.run(turn.make, "I knock.");
    store.chunk({ op_id: "op-1", text: "The door opens." });
    await settle();
    expect(store.get().busy).toBe(true);
    expect(log.reloads).toBe(0);
    turn.result.resolve(committed);
    expect(await done).toBe(true);
    expect(log.reloads).toBe(1);
    expect(store.get()).toEqual(IDLE_TURN);
  });

  it("one view call after each Turn ends, committed or failed", async () => {
    const { store, log } = harness();
    const first = attempt("op-1");
    const one = store.run(first.make, "a");
    first.result.reject(new Error("the game model is unreachable"));
    await one;
    const second = attempt("op-2");
    const two = store.run(second.make, "b");
    second.result.resolve(committed);
    await two;
    expect(log.reloads).toBe(2);
  });

  it("a failure is kept until the Player retries or dismisses it", async () => {
    const { store, log } = harness();
    const first = attempt("op-1");
    const done = store.run(first.make, "I knock.");
    first.result.reject(new Error("the game model is unreachable"));
    expect(await done).toBe(false);
    expect(store.get().failure).toBe("the game model is unreachable");
    expect(store.get().canRetry).toBe(true);
    expect(store.get().busy).toBe(false);

    // Other events leave it alone.
    store.chunk({ op_id: "op-1", text: "late chunk" });
    store.notify("Could not change the pin.");
    expect(store.get().failure).toBe("the game model is unreachable");

    // A retry clears it while the Turn runs again.
    const again = attempt("op-1");
    const retried = store.run(again.make, "I knock.");
    expect(store.get().failure).toBeNull();
    again.result.reject(new Error("still down"));
    await retried;
    expect(store.get().failure).toBe("still down");

    store.dismissFailure();
    expect(store.get().failure).toBeNull();
    expect(store.get().canRetry).toBe(false);
    expect(log.forgets).toBe(1);
  });

  it("a Refusal is a quiet line, not a failure, and offers no retry", async () => {
    const { store, log } = harness(true);
    const turn = attempt("op-1");
    const done = store.run(turn.make, "I lean closer.");
    turn.result.reject({ refusal: "An Intimate Scene cannot happen here." });
    expect(await done).toBe(false);
    expect(store.get().refusal).toBe("An Intimate Scene cannot happen here.");
    expect(store.get().failure).toBeNull();
    expect(store.get().canRetry).toBe(false);
    expect(store.get().busy).toBe(false);
    expect(log.reloads).toBe(1);

    // The next Turn clears it.
    const next = attempt("op-2");
    const sent = store.run(next.make, "I wave.");
    expect(store.get().refusal).toBeNull();
    next.result.resolve(committed);
    await sent;
    expect(store.get().refusal).toBeNull();
  });

  it("a failure with nothing to send again offers no retry", async () => {
    const { store } = harness(false);
    const turn = attempt("op-1");
    const done = store.run(turn.make, "I knock.");
    turn.result.reject(new Error("refused"));
    await done;
    expect(store.get().failure).toBe("refused");
    expect(store.get().canRetry).toBe(false);
  });

  it("a second submit while busy is refused and sends nothing", async () => {
    const { store } = harness();
    const first = attempt("op-1");
    const done = store.run(first.make, "a");
    let sent = 0;
    const second = await store.run(() => {
      sent += 1;
      return attempt("op-2").make();
    }, "b");
    expect(second).toBe(false);
    expect(sent).toBe(0);
    expect(await store.ask(() => Promise.resolve("no"))).toBe(false);
    expect(store.get().opId).toBe("op-1");
    first.result.resolve(committed);
    await done;
  });

  it("a renarration takes only the chunks of its own op", async () => {
    const { store, log } = harness();
    const call = deferred<string>();
    const done = store.renarrate(renarrateOpId(3), () => call.promise);
    store.chunk({ op_id: "op-9", text: "another Turn" });
    store.chunk({ op_id: renarrateOpId(4), text: "another renarration" });
    store.chunk({ op_id: renarrateOpId(3), text: "Told again." });
    expect(store.get().stream).toBe("Told again.");
    expect(store.get().busy).toBe(true);
    call.resolve("Told again.");
    await done;
    expect(log.reloads).toBe(1);
    expect(store.get()).toEqual(IDLE_TURN);
  });

  it("a renarration that fails leaves a notice, not a Turn failure", async () => {
    const { store } = harness();
    const done = store.renarrate(renarrateOpId(3), () => Promise.reject(new Error("timeout")));
    await done;
    expect(store.get().failure).toBeNull();
    expect(store.get().notice).toBe("Committed, narration interrupted: timeout");
  });

  it("an OOC answer keeps the story and lands after the question", async () => {
    const { store, log } = harness();
    const call = deferred<string>();
    const done = store.ask(() => call.promise);
    expect(store.get().busy).toBe(true);
    expect(store.get().stream).toBeNull();
    call.resolve("The gate shuts at dusk.");
    expect(await done).toBe(true);
    expect(store.get().answer).toBe("The gate shuts at dusk.");
    expect(log.reloads).toBe(0);
    store.dismissAnswer();
    expect(store.get().answer).toBeNull();
  });

  it("reset forgets a running Turn, and its late result changes nothing", async () => {
    const { store, log } = harness();
    const turn = attempt("op-1");
    const done = store.run(turn.make, "a");
    store.reset();
    expect(store.get()).toEqual(IDLE_TURN);
    turn.result.reject(new Error("late"));
    await done;
    expect(store.get()).toEqual(IDLE_TURN);
    expect(log.reloads).toBe(0);
  });

  it("tells each listener after a change", () => {
    const { store } = harness();
    let heard = 0;
    const off = store.subscribe(() => {
      heard += 1;
    });
    void store.run(attempt("op-1").make, "a");
    store.chunk({ op_id: "op-1", text: "x" });
    off();
    store.chunk({ op_id: "op-1", text: "y" });
    expect(heard).toBe(2);
  });
});
