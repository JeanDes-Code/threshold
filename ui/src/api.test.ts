import { describe, expect, it } from "vitest";
import {
  createApi,
  createTurnClient,
  gapsFromError,
  parseBuildProgress,
  renarrateOpId,
  turnNotice,
  type AdventureSeed,
  type Invoke,
  type TurnSummary,
} from "./api";

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

describe("turnNotice", () => {
  const summary = (narration: string | null): TurnSummary => ({
    turn_id: 7,
    roll: null,
    narration,
    narration_error: narration === null ? "the game model request timed out" : null,
    total_ms: 1,
  });

  it("tells that a Turn is committed when its narration failed", () => {
    expect(turnNotice(summary(null))).toBe(
      "Committed, narration interrupted: the game model request timed out",
    );
  });

  it("has no notice for a narrated Turn", () => {
    expect(turnNotice(summary("You wait."))).toBeNull();
  });
});

describe("createApi", () => {
  it("getPlayView calls get_play_view", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve({ story: [] } as T);
    };
    await createApi(invoke).getPlayView();
    expect(calls).toEqual([{ cmd: "get_play_view", args: undefined }]);
  });

  it("removeCampaign sends the card's id, not a path", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve(null as T);
    };
    await createApi(invoke).removeCampaign("campaign-old.sqlite");
    expect(calls).toEqual([{ cmd: "remove_campaign", args: { card_id: "campaign-old.sqlite" } }]);
  });

  it("getEntityCard sends the entity id", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve({} as T);
    };
    await createApi(invoke).getEntityCard(7);
    expect(calls).toEqual([{ cmd: "get_entity_card", args: { entity_id: 7 } }]);
  });
});

describe("renarrateOpId", () => {
  // The shell's `renarrate_op_id` builds the same text; `src-tauri/tests/play.rs` checks the prefix.
  it("names the Turn told again, apart from every Turn's own op_id", () => {
    expect(renarrateOpId(7)).toBe("__renarrate__7");
    expect(renarrateOpId(7)).not.toBe(renarrateOpId(17));
  });
});

describe("note commands", () => {
  it("note commands send their arguments", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve(undefined as T);
    };
    const api = createApi(invoke);
    await api.addNote("Ask about the ford.", { kind: "scene", turn_id: 4 });
    await api.addNote("A free note.", null);
    await api.editNote(3, "Ask Maren about the ford.");
    await api.deleteNote(3);
    await api.setPin({ kind: "belief", subject_id: 2, key: "pending_roster" }, true);
    await api.setPin({ kind: "note", note_id: 5 }, false);
    expect(calls).toEqual([
      { cmd: "add_note", args: { body: "Ask about the ford.", about: { kind: "scene", turn_id: 4 } } },
      { cmd: "add_note", args: { body: "A free note.", about: null } },
      { cmd: "edit_note", args: { note_id: 3, body: "Ask Maren about the ford." } },
      { cmd: "delete_note", args: { note_id: 3 } },
      {
        cmd: "set_pin",
        args: { target: { kind: "belief", subject_id: 2, key: "pending_roster" }, on: true },
      },
      { cmd: "set_pin", args: { target: { kind: "note", note_id: 5 }, on: false } },
    ]);
  });
});

describe("approveDraft", () => {
  const draft = { draft_id: "draft-1", state: "approved", gaps: [] };

  it("returns the approved draft", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve(draft as T);
    };
    const result = await createApi(invoke).approveDraft("draft-1");
    expect(calls).toEqual([{ cmd: "approve_draft", args: { draft_id: "draft-1" } }]);
    expect(result).toEqual({ ok: true, draft });
  });

  it("maps a gap error to a typed list", async () => {
    const gaps = [
      { kind: "age_unconfirmed", at: "entities[3].age" },
      { kind: "missing", at: "start.place" },
    ];
    const invoke: Invoke = () => Promise.reject(gaps);
    const result = await createApi(invoke).approveDraft("draft-1");
    expect(result).toEqual({ ok: false, gaps });
  });

  it("rethrows any other error", async () => {
    const invoke: Invoke = () => Promise.reject("no such draft");
    await expect(createApi(invoke).approveDraft("draft-1")).rejects.toBe("no such draft");
  });
});

describe("gapsFromError", () => {
  it("reads a list of gaps", () => {
    expect(gapsFromError([{ kind: "missing", at: "premise" }])).toEqual([
      { kind: "missing", at: "premise" },
    ]);
  });

  it("is null for a text error or a malformed list", () => {
    expect(gapsFromError("the draft is not approved")).toBeNull();
    expect(gapsFromError(new Error("boom"))).toBeNull();
    expect(gapsFromError([{ kind: "missing" }])).toBeNull();
    expect(gapsFromError([{ kind: 3, at: "premise" }])).toBeNull();
  });
});

describe("parseBuildProgress", () => {
  it("parses a progress payload", () => {
    expect(parseBuildProgress({ stage: "npc_state", done: 4, total: 9 })).toEqual({
      stage: "npc_state",
      done: 4,
      total: 9,
    });
  });

  it("keeps an unknown stage name", () => {
    expect(parseBuildProgress({ stage: "new_stage", done: 0, total: 1 })?.stage).toBe(
      "new_stage",
    );
  });

  it("is null for a malformed payload", () => {
    expect(parseBuildProgress(null)).toBeNull();
    expect(parseBuildProgress("npc_state")).toBeNull();
    expect(parseBuildProgress({ stage: "npc_state", done: "4", total: 9 })).toBeNull();
    expect(parseBuildProgress({ done: 4, total: 9 })).toBeNull();
  });
});

describe("draft commands", () => {
  it("send the argument names of the shell", async () => {
    const calls: Call[] = [];
    const invoke: Invoke = <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve(undefined as T);
    };
    const api = createApi(invoke);
    const seed = { premise: "p" } as unknown as AdventureSeed;
    await api.draftFromText("text");
    await api.saveDraft("draft-1", seed, []);
    const sefa = { index: 1, name: "Sefa Lind", kind: "npc" } as const;
    await api.confirmAge("draft-1", sefa);
    await api.renameDraftEntity("draft-1", sefa, "Sefa Marsh");
    await api.startBuild("draft-1");
    await api.cancelBuild();
    await api.begin();
    await api.deleteDraft("draft-1");
    await api.getBuildDebug("draft-1");
    expect(calls).toEqual([
      { cmd: "draft_from_text", args: { text: "text" } },
      // No list of ages: the core owns each confirmation.
      { cmd: "save_draft", args: { draft_id: "draft-1", seed, knowledge: [] } },
      { cmd: "confirm_age", args: { draft_id: "draft-1", entry: sefa } },
      {
        cmd: "rename_draft_entity",
        args: { draft_id: "draft-1", entry: sefa, to: "Sefa Marsh" },
      },
      { cmd: "start_build", args: { draft_id: "draft-1" } },
      { cmd: "cancel_build", args: undefined },
      { cmd: "begin", args: undefined },
      { cmd: "delete_draft", args: { draft_id: "draft-1" } },
      { cmd: "get_build_debug", args: { draft_id: "draft-1" } },
    ]);
  });
});
