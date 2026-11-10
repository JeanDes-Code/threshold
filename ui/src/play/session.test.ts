import { describe, expect, it } from "vitest";
import type { PlayView } from "../api";
import { playView } from "../fixtures/playView";
import { createPlaySession } from "./session";

function countingApi() {
  const calls: string[] = [];
  const api = {
    getPlayView: (): Promise<PlayView> => {
      calls.push("get_play_view");
      return Promise.resolve(playView);
    },
  };
  return { calls, api };
}

describe("createPlaySession", () => {
  it("one view call after open, after each Turn result and after each note command", async () => {
    const { calls, api } = countingApi();
    const session = createPlaySession(api);
    expect(calls).toEqual([]);

    expect(await session.opened()).toBe(playView);
    expect(calls).toHaveLength(1);

    await session.turnEnded();
    await session.turnEnded();
    expect(calls).toHaveLength(3);

    await session.noteChanged();
    expect(calls).toEqual(["get_play_view", "get_play_view", "get_play_view", "get_play_view"]);
  });
});
