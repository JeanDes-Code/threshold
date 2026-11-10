// Invented entity cards that match the Play fixture, for the preview and the
// UI tests. Every name and line here is made up.

import type { EntityCard, FactLine, NameRef } from "../api";
import { playView } from "./playView";

const who = (entityId: number): NameRef => {
  const found =
    playView.known.find((line) => line.who.entity_id === entityId)?.who ??
    playView.here.find((ref) => ref.entity_id === entityId);
  if (!found) throw new Error(`no entity ${entityId} in the Play fixture`);
  return found;
};

const fact = (subject: NameRef, key: string, text: string, told_by: NameRef | null = null): FactLine => ({
  belief: { subject_id: subject.entity_id, key },
  subject,
  text,
  open: key.startsWith("pending_"),
  told_by,
});

const card = (entityId: number, facts: (subject: NameRef) => FactLine[], people_here: NameRef[] = []): EntityCard => {
  const ref = who(entityId);
  const line = playView.known.find((l) => l.who.entity_id === entityId);
  return {
    who: ref,
    other_names: line?.other_names ?? [],
    here: playView.here.some((h) => h.entity_id === entityId) || (line?.here ?? false),
    facts: facts(ref),
    people_here,
    notes: [],
    pinned: false,
  };
};

const maren = who(2);

/** The cards the PC can open, by entity id. Any other id is refused. */
export const entityCards: Record<number, EntityCard> = {
  2: card(2, (s) => [
    fact(s, "role", "Maren is the night clerk of the Harbour Office."),
    fact(s, "mood", "Maren mood: wary"),
    fact(s, "pending_close", "Maren will close the office at the second bell."),
  ]),
  3: card(3, (s) => [
    fact(s, "role", "Orsel is the foreman of the night porters."),
    fact(s, "pending_roster", "Orsel must bring the night roster to the Salt Guild.", s),
  ]),
  4: card(
    4,
    (s) => [
      fact(s, "condition", "The Harbour Office keeps a lamp lit all night."),
      fact(s, "rule", "No ledger leaves the counter of the Harbour Office.", maren),
    ],
    [maren, who(3)],
  ),
  5: card(5, (s) => [
    fact(s, "holds", "The Salt Guild holds the debts of half the harbour."),
    fact(s, "wants", "The Salt Guild wants the sealed manifest before the bell."),
  ]),
  6: card(6, (s) => [fact(s, "use", "The night porters wait for work on the Lantern Steps.")]),
  7: card(7, () => []),
  8: card(8, (s) => [
    fact(s, "commands", "Captain Hale commands the Gull."),
    fact(s, "pending_arrival", "Captain Hale swore the Gull would beat the weather.", maren),
  ]),
};

/** The refusal of `get_entity_card`, as the shell words it. */
export const UNKNOWN_ENTITY = "the PC does not know this one";

/** `get_entity_card` over the fixture. */
export function fixtureCard(entityId: number): Promise<EntityCard> {
  const found = entityCards[entityId];
  return found ? Promise.resolve(found) : Promise.reject(UNKNOWN_ENTITY);
}
