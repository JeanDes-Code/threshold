// Pure rules of the home screen and of the app's screens. The core reads
// each Campaign file; these only word and order what it sends.

import type { CampaignCard, DraftLine } from "../api";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Played just now", "5 minutes ago", "yesterday", "3 days ago", then a date. */
export function lastPlayedText(ms: number, now: number): string {
  const ago = now - ms;
  if (ms <= 0) return "never played";
  if (ago < MINUTE) return "played just now";
  if (ago < HOUR) {
    const n = Math.floor(ago / MINUTE);
    return `played ${n} minute${n === 1 ? "" : "s"} ago`;
  }
  if (ago < DAY) {
    const n = Math.floor(ago / HOUR);
    return `played ${n} hour${n === 1 ? "" : "s"} ago`;
  }
  if (ago < 2 * DAY) return "played yesterday";
  if (ago < 7 * DAY) return `played ${Math.floor(ago / DAY)} days ago`;
  return `played on ${dayText(ms)}`;
}

/** The card's count line: "Not begun yet", "1 Turn", "12 Turns", or its problem. */
export function cardLine(card: CampaignCard): string {
  if (card.problem !== null) return card.problem;
  if (card.needs_begin) return "Not begun yet";
  return `${card.turn_count} Turn${card.turn_count === 1 ? "" : "s"}`;
}

/** The file name of a card's path, for a card the core could not read. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * When a Campaign was created, in ms since the epoch, from its file name
 * (`campaign-<nanoseconds>.sqlite`, written by the shell); null for any
 * other name.
 */
export function createdMs(path: string): number | null {
  const found = /^campaign-(\d{10,})\.sqlite$/.exec(fileName(path));
  if (!found) return null;
  const ms = Math.floor(Number(found[1]) / 1_000_000);
  return Number.isFinite(ms) && ms > 0 ? ms : null;
}

/** "11 Nov 2026": a day as the home screen writes it, on the Player's local clock. */
export function dayText(ms: number): string {
  const date = new Date(ms);
  return `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()}`;
}

/** "14:05": a time of day on the Player's local clock. */
export function timeText(ms: number): string {
  const date = new Date(ms);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/**
 * What tells two Campaign cards apart beyond the PC's name (QA, bug pass 4:
 * two cards with one PC differed only by their Turn count): the day the
 * Campaign was started, when the file name gives it.
 */
export function startedText(path: string): string | null {
  const ms = createdMs(path);
  // The time too: two Campaigns of one PC started on one day read apart
  // (bug pass 5).
  return ms === null ? null : `started ${dayText(ms)}, ${timeText(ms)}`;
}

/**
 * The title of a card the core could not read: a plain label, never a raw
 * file name. It holds the time too, so two such files of one day read apart.
 */
export function problemTitle(path: string): string {
  const ms = createdMs(path);
  return ms === null ? "A Campaign file" : `A Campaign started ${dayText(ms)}, ${timeText(ms)}`;
}

/** The Seed Drafts the home screen lists: the ones not built yet. */
export function unfinishedDrafts(drafts: DraftLine[]): DraftLine[] {
  return drafts.filter((d) => d.state !== "built");
}

export type ModelState = "ready" | "warming" | "down";

/** The model status the shell reports: "ready", "warming", or an error text. */
export function modelState(status: string): ModelState {
  if (status === "ready") return "ready";
  if (status === "warming") return "warming";
  return "down";
}

export function modelText(status: string): string {
  const state = modelState(status);
  if (state === "ready") return "The GM model is ready";
  if (state === "warming") return "The GM model is warming up";
  return `The GM model is not ready: ${status}`;
}

/** The screens of an open Campaign. */
export type CampaignTab = "play" | "journal" | "character" | "debug";

/** One screen at a time: a new screen replaces the last, nothing stacks. */
export type AppScreen =
  | { kind: "home" }
  | { kind: "new" }
  | { kind: "debug" }
  | { kind: "campaign"; tab: CampaignTab };

export type Go =
  | { to: "home" }
  | { to: "new" }
  | { to: "opened" }
  | { to: "tab"; tab: CampaignTab }
  | { to: "debug" };

/**
 * The next screen. Opening a Campaign shows its story. The debug view needs
 * debug mode; a Campaign tab needs an open Campaign.
 */
export function navigate(screen: AppScreen, go: Go, ctx: { debug: boolean; open: boolean }): AppScreen {
  switch (go.to) {
    case "home":
      return { kind: "home" };
    case "new":
      return { kind: "new" };
    case "opened":
      return { kind: "campaign", tab: "play" };
    case "debug":
      if (!ctx.debug) return screen;
      return ctx.open ? { kind: "campaign", tab: "debug" } : { kind: "debug" };
    case "tab":
      if (!ctx.open) return screen;
      if (go.tab === "debug" && !ctx.debug) return screen;
      return { kind: "campaign", tab: go.tab };
  }
}

/** When debug mode goes off, a debug screen gives way. */
export function withoutDebug(screen: AppScreen): AppScreen {
  if (screen.kind === "debug") return { kind: "home" };
  if (screen.kind === "campaign" && screen.tab === "debug") return { kind: "campaign", tab: "play" };
  return screen;
}
