import { describe, expect, it } from "vitest";
import { committedLine, createApi, createTurnClient, type Invoke, type TurnSummary } from "./api";

interface Call {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

/** An `invoke` that records calls and answers from a queue of outcomes. */
function fakeInvoke(outcomes: ("ok" | "fail")[]) {
  const calls: Call[] = [];
  const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    const outcome = outcomes.shift() ?? "ok";
    if (outcome === "fail") return Promise.reject(new Error("the game model is unreachable"));
    return Promise.resolve({
      turn_id: calls.length,
      roll: null,
      narration: "ok",
      narration_error: null,
      total_ms: 1,
    } as T);
  };
  return { calls, invoke };
}

function counter() {
  let n = 0;
  return () => `op-${++n}`;
}

describe("createTurnClient", () => {
  it("builds a fresh op_id per submit", async () => {
    const { calls, invoke } = fakeInvoke(["ok", "ok"]);
    const client = createTurnClient(invoke, counter());
    await client.submit("I look around.").result;
    await client.submit("I greet Orsel.").result;
    expect(calls).toEqual([
      { cmd: "submit_intent", args: { op_id: "op-1", text: "I look around." } },
      { cmd: "submit_intent", args: { op_id: "op-2", text: "I greet Orsel." } },
    ]);
  });

  it("reuses the op_id on retry", async () => {
    const { calls, invoke } = fakeInvoke(["fail", "ok"]);
    const client = createTurnClient(invoke, counter());
    const first = client.submit("I look around.");
    await expect(first.result).rejects.toThrow("unreachable");
    expect(client.pending()?.opId).toBe("op-1");

    const retry = client.retry();
    expect(retry?.opId).toBe("op-1");
    await retry?.result;
    expect(calls[1]).toEqual({
      cmd: "submit_intent",
      args: { op_id: "op-1", text: "I look around." },
    });
    expect(client.pending()).toBeNull();
    expect(client.retry()).toBeNull();
  });

  it("retries a Correction with its own op_id and arguments", async () => {
    const { calls, invoke } = fakeInvoke(["ok", "fail", "ok"]);
    const client = createTurnClient(invoke, counter());
    await client.submit("I insult Orsel.").result;
    const fix = client.correct("I only bow to Orsel.", "I meant a bow");
    await expect(fix.result).rejects.toThrow();
    await client.retry()?.result;
    const args = { op_id: "op-2", clarified: "I only bow to Orsel.", why: "I meant a bow" };
    expect(calls.slice(1)).toEqual([
      { cmd: "correct_last", args },
      { cmd: "correct_last", args },
    ]);
  });

  it("drops a pending retry when a new Intent is submitted", async () => {
    const { calls, invoke } = fakeInvoke(["fail", "ok"]);
    const client = createTurnClient(invoke, counter());
    await expect(client.submit("one").result).rejects.toThrow();
    await client.submit("two").result;
    expect(calls[1]?.args).toEqual({ op_id: "op-2", text: "two" });
    expect(client.pending()).toBeNull();
  });

  it("reset() clears the pending Turn so retry() returns null", async () => {
    const { calls, invoke } = fakeInvoke(["fail"]);
    const client = createTurnClient(invoke, counter());
    await expect(client.submit("I insult Orsel.").result).rejects.toThrow();
    expect(client.pending()).not.toBeNull();

    client.reset();
    expect(client.pending()).toBeNull();
    expect(client.retry()).toBeNull();
    expect(calls).toHaveLength(1);
  });

  it("reset() during a Turn keeps its late result from reviving it", async () => {
    let fail: (e: Error) => void = () => undefined;
    const invoke: Invoke = <T,>() => new Promise<T>((_, reject) => (fail = reject));
    const client = createTurnClient(invoke, counter());
    const attempt = client.submit("I wait.");
    client.reset();
    fail(new Error("timeout"));
    await expect(attempt.result).rejects.toThrow();
    expect(client.pending()).toBeNull();
  });
});

describe("committedLine", () => {
  const summary = (narration: string | null): TurnSummary => ({
    turn_id: 7,
    roll: null,
    narration,
    narration_error: narration === null ? "the game model request timed out" : null,
    total_ms: 1,
  });

  it("keeps a Turn whose narration failed, without narration", () => {
    const { line, notice } = committedLine(summary(null), "play", "I wait.");
    expect(line).toEqual({
      turn_id: 7,
      kind: "play",
      intent: "I wait.",
      narration: null,
      roll: null,
      reverted: false,
    });
    expect(notice).toBe("Committed, narration interrupted: the game model request timed out");
  });

  it("has no notice for a narrated Turn", () => {
    const { line, notice } = committedLine(summary("You wait."), "play", "I wait.");
    expect(line.narration).toBe("You wait.");
    expect(notice).toBeNull();
  });
});

describe("createApi", () => {
  it("reads the campaign summary", async () => {
    const calls: string[] = [];
    const invoke: Invoke = <T,>(cmd: string) => {
      calls.push(cmd);
      return Promise.resolve({ turns: [] } as T);
    };
    await createApi(invoke).summary();
    expect(calls).toEqual(["campaign_summary"]);
  });
});
