// Dev only: the New Campaign review on an invented draft, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=review while `npm run dev` runs.
// `&step=source` shows a Source Text the shell refused; `&debug=1` turns
// debug mode on.

import { useState } from "react";
import type { AgeLine, DraftView, Gap, SeedEntity } from "../api";
import { confirmedAges, draftedAges, otherGaps, seedDraftView, shortSource } from "../fixtures/seedDraft";
import { isCharacter } from "../newCampaign";
import ScreenFrame from "./ScreenFrame";
import SeedReview, { type ReviewClient } from "./SeedReview";
import SetupSteps from "./SetupSteps";
import SourceStep from "./SourceStep";

/** A rough stand-in for the core, enough to click through the review. */
function fakeClient(): { client: ReviewClient; first: DraftView } {
  let view = seedDraftView;
  const drafted = { ...draftedAges };
  const confirmed: Record<string, number | undefined> = { ...confirmedAges };

  const refresh = (): DraftView => {
    const rows: [string, string, number | null][] = [
      ["pc.age", view.seed.pc.name, view.seed.pc.age],
      ...view.seed.entities.flatMap((e: SeedEntity, i): [string, string, number | null][] =>
        isCharacter(e.kind) ? [[`entities[${i}].age`, e.name, e.age]] : [],
      ),
    ];
    const ages: AgeLine[] = rows.map(([at, name, age]) => {
      const mark = drafted[name];
      const source = !mark || mark.age !== age ? "edited" : mark.stated ? "from_text" : "proposed";
      const index = at === "pc.age" ? null : Number(/\d+/.exec(at)?.[0]);
      const kind = index === null ? "pc" : (view.seed.entities[index]?.kind ?? "npc");
      return { name, index, kind, source, confirmed: age !== null && confirmed[name] === age };
    });
    const names = new Set(view.seed.entities.map((e) => e.name));
    const others = otherGaps.filter(
      (g) => g.at !== "start.present[2]" || !names.has(view.seed.start.present[2] ?? ""),
    );
    const ageGaps: Gap[] = rows
      .filter((_, i) => !ages[i]?.confirmed)
      .map(([at]) => ({ kind: "age_unconfirmed", at }));
    view = { ...view, ages, gaps: [...ageGaps, ...others] };
    return view;
  };
  const first = refresh();

  const later = <T,>(value: T) => new Promise<T>((resolve) => setTimeout(() => resolve(value), 120));

  const client: ReviewClient = {
    saveDraft: (_, seed, knowledge) => {
      view = { ...view, seed, knowledge, state: "draft" };
      return later(refresh());
    },
    confirmAge: (_, { name }) => {
      const age = name === view.seed.pc.name ? view.seed.pc.age : view.seed.entities.find((e) => e.name === name)?.age;
      if (age !== null && age !== undefined) confirmed[name] = age;
      return later(refresh());
    },
    renameDraftEntity: (_, { name: from }, to) => {
      const taken = [view.seed.pc.name, ...view.seed.entities.map((e) => e.name)].some(
        (n) => n !== from && n.toLowerCase() === to.trim().toLowerCase(),
      );
      if (taken) return Promise.reject(`Another entry already uses the name “${to.trim()}”. Choose another name.`);
      const swap = (n: string) => (n === from ? to.trim() : n);
      const { seed } = view;
      view = {
        ...view,
        seed: {
          ...seed,
          pc: {
            ...seed.pc,
            name: swap(seed.pc.name),
            other_names: seed.pc.other_names.map((o) => ({ ...o, known_to: o.known_to.map(swap) })),
          },
          entities: seed.entities.map((e) => ({
            ...e,
            name: swap(e.name),
            other_names: e.other_names.map((o) => ({ ...o, known_to: o.known_to.map(swap) })),
          })),
          start: { ...seed.start, place: swap(seed.start.place), present: seed.start.present.map(swap) },
        },
        knowledge: view.knowledge.map((k) => ({ ...k, subject: swap(k.subject), text: k.text.split(from).join(to.trim()) })),
      };
      const mark = drafted[from];
      if (mark) drafted[to.trim()] = mark;
      confirmed[to.trim()] = confirmed[from];
      return later(refresh());
    },
    approveDraft: () =>
      later(view.gaps.length > 0 ? { ok: false as const, gaps: view.gaps } : { ok: true as const, draft: view }),
  };
  return { client, first };
}

export default function ReviewPreview() {
  const params = new URLSearchParams(window.location.search);
  const [{ client, first }] = useState(fakeClient);
  const [text, setText] = useState(shortSource);
  const source = params.get("step") === "source";
  return (
    <div className="preview">
      <div className="preview__strip">
        <span className="preview__label">Preview · fixture data</span>
      </div>
      <ScreenFrame label="New Campaign" title="From a document" onBack={() => undefined} backText="Home">
        {source ? (
          <>
            <SetupSteps current="source" />
            <SourceStep
              text={text}
              onText={setText}
              onFile={() => undefined}
              onDraft={() => undefined}
              busy={false}
              running={null}
              error={`This Source Text is too short to hold a premise and your character: it has ${[...text.trim()].length} characters, and a draft needs at least 400. Add more of your document.`}
            />
          </>
        ) : (
          <>
            <SetupSteps current="review" />
            <SeedReview
              view={first}
              client={client}
              debug={params.get("debug") === "1"}
              onApproved={() => undefined}
              onBack={() => undefined}
            />
          </>
        )}
      </ScreenFrame>
    </div>
  );
}
