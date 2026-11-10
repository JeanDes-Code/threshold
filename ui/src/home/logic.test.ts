import { describe, expect, it } from "vitest";
import type { CampaignCard } from "../api";
import {
  cardLine,
  fileName,
  lastPlayedText,
  modelState,
  modelText,
  navigate,
  unfinishedDrafts,
  withoutDebug,
  type AppScreen,
} from "./logic";

const MIN = 60_000;
const now = Date.UTC(2026, 10, 10, 18, 0);

const card = (extra: Partial<CampaignCard> = {}): CampaignCard => ({
  db_path: "/data/campaigns/campaign-1.sqlite",
  pc_name: "Ada Quell",
  premise: "A courier owes the Salt Guild more than she can pay.",
  last_played_ms: now,
  turn_count: 12,
  needs_begin: false,
  problem: null,
  ...extra,
});

describe("home cards", () => {
  it("last played reads in words", () => {
    expect(lastPlayedText(now - 20_000, now)).toBe("played just now");
    expect(lastPlayedText(now - MIN, now)).toBe("played 1 minute ago");
    expect(lastPlayedText(now - 45 * MIN, now)).toBe("played 45 minutes ago");
    expect(lastPlayedText(now - 3 * 60 * MIN, now)).toBe("played 3 hours ago");
    expect(lastPlayedText(now - 30 * 60 * MIN, now)).toBe("played yesterday");
    expect(lastPlayedText(now - 4 * 1440 * MIN, now)).toBe("played 4 days ago");
    expect(lastPlayedText(Date.UTC(2026, 9, 2, 9, 0), now)).toBe("played on 2 Oct 2026");
    expect(lastPlayedText(0, now)).toBe("never played");
  });

  it("a card line names the Turn count", () => {
    expect(cardLine(card())).toBe("12 Turns");
    expect(cardLine(card({ turn_count: 1 }))).toBe("1 Turn");
    expect(cardLine(card({ turn_count: 0, needs_begin: true }))).toBe("Not begun yet");
    const old = "made by an older version of Threshold: import its Source Text again";
    expect(cardLine(card({ problem: old }))).toBe(old);
  });

  it("a card without a name shows its file name", () => {
    expect(fileName("/data/campaigns/campaign-1.sqlite")).toBe("campaign-1.sqlite");
    expect(fileName("C:\\data\\campaign-2.sqlite")).toBe("campaign-2.sqlite");
  });

  it("home lists the drafts that are not built", () => {
    const drafts = unfinishedDrafts([
      { draft_id: "a", title: "A", state: "draft" },
      { draft_id: "b", title: "B", state: "built" },
      { draft_id: "c", title: "C", state: "approved" },
    ]);
    expect(drafts.map((d) => d.draft_id)).toEqual(["a", "c"]);
  });

  it("the model status reads as a state and a sentence", () => {
    expect(modelState("ready")).toBe("ready");
    expect(modelState("warming")).toBe("warming");
    expect(modelState("the game model is unreachable")).toBe("down");
    expect(modelText("warming")).toBe("The GM model is warming up");
    expect(modelText("timeout")).toBe("The GM model is not ready: timeout");
  });
});

describe("navigate", () => {
  const off = { debug: false, open: false };
  const open = { debug: false, open: true };

  it("the app opens on home and opening a Campaign replaces it: nothing stacks", () => {
    const home: AppScreen = { kind: "home" };
    expect(navigate(home, { to: "opened" }, open)).toEqual({ kind: "campaign", tab: "play" });
    // Reopening from the Journal of another Campaign lands on the story, alone.
    expect(navigate({ kind: "campaign", tab: "journal" }, { to: "opened" }, open)).toEqual({
      kind: "campaign",
      tab: "play",
    });
    expect(navigate({ kind: "new" }, { to: "opened" }, open)).toEqual({ kind: "campaign", tab: "play" });
    expect(navigate({ kind: "campaign", tab: "play" }, { to: "home" }, open)).toEqual(home);
  });

  it("a Campaign tab needs an open Campaign, the debug view needs debug mode", () => {
    expect(navigate({ kind: "home" }, { to: "tab", tab: "journal" }, off)).toEqual({ kind: "home" });
    expect(navigate({ kind: "campaign", tab: "play" }, { to: "tab", tab: "debug" }, open)).toEqual({
      kind: "campaign",
      tab: "play",
    });
    expect(navigate({ kind: "home" }, { to: "debug" }, { debug: true, open: false })).toEqual({ kind: "debug" });
    expect(navigate({ kind: "campaign", tab: "play" }, { to: "debug" }, { debug: true, open: true })).toEqual({
      kind: "campaign",
      tab: "debug",
    });
    expect(withoutDebug({ kind: "debug" })).toEqual({ kind: "home" });
    expect(withoutDebug({ kind: "campaign", tab: "debug" })).toEqual({ kind: "campaign", tab: "play" });
    expect(withoutDebug({ kind: "campaign", tab: "journal" })).toEqual({ kind: "campaign", tab: "journal" });
  });
});
