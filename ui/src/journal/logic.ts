// Pure rules of the Journal screen. The core writes every entry; these only
// order and group what it sends.

import {
  clockText,
  type Act,
  type FactLine,
  type JournalView,
  type NameRef,
  type SceneEntry,
  type Target,
} from "../api";

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

/** How an Open Item ended, as the Journal shows it: "Refused · <item text>". */
export function closedText({ outcome, text }: { outcome: string; text: string }): string {
  return `${outcome} · ${text}`;
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

/** A NameRef the Journal holds for an entity, from any of its lists. */
function nameIn(view: JournalView, entityId: number): NameRef | null {
  const lines = [
    ...view.scenes.flatMap((s) => s.learned),
    ...view.open,
    ...view.closed.map((c) => c.item),
    ...view.known_before,
  ];
  const refs = [
    ...view.scenes.flatMap((s) => s.present),
    ...lines.map((l) => l.subject),
    ...lines.flatMap((l) => (l.told_by ? [l.told_by] : [])),
  ];
  return refs.find((r) => r.entity_id === entityId) ?? null;
}

/** What a Note is about, in words: a Scene, an act, a fact, a person or place. */
export function aboutText(view: JournalView, about: Target | null): string {
  if (about === null) return "Free note";
  switch (about.kind) {
    case "scene": {
      const where = sceneOfTurn(view, about.turn_id);
      return where ? `Scene ${where.number} · ${placeTitle(where.scene.place)}` : "A scene";
    }
    case "act": {
      const act = view.scenes.flatMap((s) => s.acts).find((a) => a.turn_id === about.turn_id);
      return act ? `What you did: ${actText(act)}` : "Something you did";
    }
    case "belief": {
      const line = [
        ...view.scenes.flatMap((s) => s.learned),
        ...view.open,
        ...view.closed.map((c) => c.item),
        ...view.known_before,
      ].find((l) => l.belief.subject_id === about.subject_id && l.belief.key === about.key);
      return line ? `Fact: ${line.text}` : "A fact";
    }
    case "entity": {
      const who = nameIn(view, about.entity_id);
      return who?.name ? `About ${who.name}` : "About someone you know";
    }
    case "note":
      return "A note";
  }
}

/** What the Journal shows for an act: its Intent, or the neutral line of a faded Turn. */
export function actText(act: Act): string {
  return act.faded ?? act.intent;
}
