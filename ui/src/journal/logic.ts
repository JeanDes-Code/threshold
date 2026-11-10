// Pure rules of the Journal screen. The core writes every entry; these only
// order and group what it sends.

import { clockText, type Act, type FactLine, type JournalView, type NameRef, type SceneEntry } from "../api";

/** A Scene and its number in the Campaign, the first Scene being 1. */
export interface NumberedScene {
  scene: SceneEntry;
  number: number;
}

/** The timeline: the latest Scene first. */
export function scenesNewestFirst(view: JournalView): NumberedScene[] {
  return view.scenes.map((scene, i) => ({ scene, number: i + 1 })).reverse();
}

/** The Decisions part: each decision act with its Scene, the latest first. */
export function decisions(view: JournalView): { act: Act; scene: SceneEntry }[] {
  return view.scenes
    .flatMap((scene) => scene.acts.filter((act) => act.decision).map((act) => ({ act, scene })))
    .reverse();
}

/** The Scene that holds a Turn, by its first Turn or one of its acts. */
export function sceneOfTurn(view: JournalView, turnId: number): NumberedScene | null {
  const i = view.scenes.findIndex(
    (scene) => scene.turn_id === turnId || scene.acts.some((act) => act.turn_id === turnId),
  );
  const scene = view.scenes[i];
  return scene ? { scene, number: i + 1 } : null;
}

/** "Day 1, 08:05", "Day 1, 08:05 – 09:40", or across days "Day 1, 23:00 – Day 2, 01:00". */
export function clockSpan(from: number, to: number): string {
  const start = clockText(from);
  if (to <= from) return start;
  const end = clockText(to);
  const sameDay = Math.floor(from / 1440) === Math.floor(to / 1440);
  return `${start} – ${sameDay ? end.slice(end.indexOf(", ") + 2) : end}`;
}

/** A Scene's place as a title: its first letter upper case, "Somewhere" when empty. */
export function placeTitle(place: string): string {
  const text = place.trim();
  return text === "" ? "Somewhere" : text.charAt(0).toUpperCase() + text.slice(1);
}

/** Fact lines grouped by their subject, in the order each subject first comes. */
export function bySubject(lines: FactLine[]): { subject: NameRef; lines: FactLine[] }[] {
  const groups: { subject: NameRef; lines: FactLine[] }[] = [];
  for (const line of lines) {
    const group = groups.find((g) => g.subject.entity_id === line.subject.entity_id);
    if (group) group.lines.push(line);
    else groups.push({ subject: line.subject, lines: [line] });
  }
  return groups;
}
