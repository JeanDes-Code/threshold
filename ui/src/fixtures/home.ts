// Invented Campaign cards and drafts for the home preview and the UI tests.
// Every name and premise here is made up.

import type { CampaignCard, DraftLine } from "../api";

/** The preview's clock: the cards' times read against it. */
export const homeNow = Date.UTC(2026, 10, 10, 19, 30);

const HOUR = 3_600_000;

export const homeCards: CampaignCard[] = [
  {
    id: "campaign-3.sqlite",
    db_path: "/data/campaigns/campaign-3.sqlite",
    pc_name: "Ada Quell",
    premise:
      "A harbour courier owes the Salt Guild more than she can pay, and a ship that should have docked two tides ago carries the only way out.",
    last_played_ms: homeNow - 2 * HOUR,
    turn_count: 48,
    needs_begin: false,
    problem: null,
  },
  {
    id: "campaign-2.sqlite",
    db_path: "/data/campaigns/campaign-2.sqlite",
    pc_name: "Wren",
    premise: "An outsider scribe is hired to copy the town's charters, and one charter is missing a page.",
    last_played_ms: homeNow - 3 * 24 * HOUR,
    turn_count: 12,
    needs_begin: false,
    problem: null,
  },
  {
    id: "campaign-4.sqlite",
    db_path: "/data/campaigns/campaign-4.sqlite",
    pc_name: "Mira Holt",
    premise: "A lighthouse keeper's daughter comes home to Gull Point the week the lamp goes dark.",
    last_played_ms: homeNow - 26 * HOUR,
    turn_count: 0,
    needs_begin: true,
    problem: null,
  },
  {
    id: "campaign-1.sqlite",
    db_path: "/data/campaigns/campaign-1.sqlite",
    pc_name: "",
    premise: "",
    last_played_ms: homeNow - 40 * 24 * HOUR,
    turn_count: 0,
    needs_begin: false,
    problem: "made by an older version of Threshold: import its Source Text again",
  },
];

export const homeDrafts: DraftLine[] = [
  { draft_id: "d-1", title: "The orchard feud of Low Ashby", state: "draft" },
  { draft_id: "d-2", title: "Salt and silence on the Varn road", state: "approved" },
  { draft_id: "d-3", title: "A built one, not listed", state: "built" },
];
