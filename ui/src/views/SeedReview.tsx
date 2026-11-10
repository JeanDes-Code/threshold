import { useEffect, useState, type ReactNode } from "react";
import {
  errorText,
  type AdventureSeed,
  type AgeLine,
  type Api,
  type Cut,
  type DraftView,
  type EntityKind,
  type Gap,
  type KnownFact,
  type OpenMode,
  type PcSeed,
  type SeedEntity,
  type StartSeed,
} from "../api";
import {
  SECTIONS,
  addEntry,
  afterFlush,
  ageLabel,
  ageLineOf,
  ageStatus,
  ageTextMatches,
  agesLeft,
  canApprove as approvable,
  confirmRow,
  createSaver,
  cutSections,
  cutSentence,
  dropTyped,
  fieldId,
  focusTarget,
  formOfView,
  gapSentence,
  gapsAt,
  isCharacter,
  nameChange,
  removeAt,
  removeEntry,
  renamed,
  replaceAt,
  rowPath,
  saveArgs,
  savedName,
  sectionOf,
  shownName,
  typeName,
  type ReviewForm,
  type Row,
  type SectionId,
} from "../newCampaign";
import { api } from "../tauri";
import { AgeInput, ListInput, NameInput, OtherNames, TextInput } from "./fields";

/** The commands the review calls. The dev preview passes a stand-in. */
export type ReviewClient = Pick<Api, "saveDraft" | "confirmAge" | "renameDraftEntity" | "approveDraft">;

interface Props {
  view: DraftView;
  /** The draft is approved: the World Build may start. */
  onApproved: (view: DraftView) => void;
  onBack: () => void;
  /** Debug mode shows the internal fields of each fact. */
  debug?: boolean;
  client?: ReviewClient;
}

const PLACE_KINDS: [EntityKind, string][] = [
  ["place", "place"],
  ["faction", "group"],
  ["item", "object"],
];
const KINDS: [EntityKind, string][] = [["npc", "person"], ...PLACE_KINDS];
const MODES: [OpenMode, string][] = [
  ["delegated", "the GM decides"],
  ["discover", "found in play"],
  ["to_agree", "agree before play"],
];
const FACT_FIELDS = [
  ["subject", "About", "Who or what the fact is about: a name from the draft."],
  ["key", "Detail", "Which detail, in one or two words: job, home, owner."],
  ["value", "Value", "The detail itself, in a few words."],
] as const;

/**
 * The Seed Draft review: a guided check, part by part. Every edit is saved
 * after a pause in typing and brings back the fresh gaps. A name commits
 * through the core's rename, an age through the core's confirmation.
 * "Approve" waits for no gap and no pending save.
 */
export default function SeedReview({ view, onApproved, onBack, debug = false, client = api }: Props) {
  const [form, setForm] = useState<ReviewForm>(() => formOfView(view));
  const [gaps, setGaps] = useState<Gap[]>(view.gaps);
  const [ages, setAges] = useState<AgeLine[]>(view.ages);
  const [cut] = useState<Cut[]>(view.cut);
  const [state, setState] = useState(view.state);
  const [pending, setPending] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  // The text of each age field, by its path, while it is not the stored age.
  const [ageTexts, setAgeTexts] = useState<Partial<Record<string, string | null>>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nameErrors, setNameErrors] = useState<Partial<Record<string, string>>>({});
  const [approving, setApproving] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [editing, setEditing] = useState<ReadonlySet<number>>(() => new Set());
  const [adding, setAdding] = useState<"character" | "place" | null>(null);
  // Set and read in event handlers only: a rename on Enter must not run
  // again on the blur that follows, before React renders the busy state.
  const [guard] = useState(createGuard);
  const draftId = view.draft_id;

  /** What the store answered: gaps, ages and state. The form stays the Player's. */
  const apply = (saved: DraftView) => {
    setGaps(saved.gaps);
    setAges(saved.ages);
    setState(saved.state);
  };

  // One saver per mount: the parent gives each draft its own key.
  const [saver] = useState(() =>
    createSaver<ReviewForm, DraftView>({
      delayMs: 600,
      save: (f) => {
        const { seed, knowledge } = saveArgs(f);
        return client.saveDraft(draftId, seed, knowledge);
      },
      onResult: (saved) => {
        apply(saved);
        setSaveFailed(false);
        setError(null);
      },
      onError: (e) => {
        setSaveFailed(true);
        setError(`Could not save: ${errorText(e)}`);
      },
      onPending: setPending,
    }),
  );
  // On leaving, the last edit is still saved.
  useEffect(() => {
    saver.start();
    return () => saver.stop();
  }, [saver]);

  // While approval runs, no edit may return the draft to `Draft`.
  const locked = approving || leaving;

  /** `now`: a click, saved at once; typing waits for a pause. No edit while the core works. */
  const update = (change: (f: ReviewForm) => ReviewForm, now = false) => {
    if (locked || guard.held()) return;
    const next = change(form);
    setForm(next);
    saver.edit(next, now);
  };

  /**
   * Runs one core action. `run` gets the saver's flush: each action flushes
   * first, so the core renames or confirms what the Player sees. Returns the
   * error text.
   */
  const core = async (
    run: (flush: () => Promise<void>) => Promise<DraftView>,
    then?: (v: DraftView) => void,
  ) => {
    if (locked || !guard.enter()) return null;
    setBusy(true);
    try {
      const result = await run(() => saver.flush());
      setSaveFailed(false);
      apply(result);
      then?.(result);
      return null;
    } catch (e) {
      return errorText(e);
    } finally {
      guard.leave();
      setBusy(false);
    }
  };

  const seed = form.seed;
  const setSeed = (patch: Partial<AdventureSeed>, now = false) =>
    update((f) => ({ ...f, seed: { ...f.seed, ...patch } }), now);
  const setStart = (patch: Partial<StartSeed>) => setSeed({ start: { ...seed.start, ...patch } });
  const setPc = (patch: Partial<PcSeed>, now = false) =>
    update((f) => ({ ...f, seed: { ...f.seed, pc: { ...f.seed.pc, ...patch } } }), now);
  const setEntity = (i: number, patch: Partial<SeedEntity>, now = false) =>
    update(
      (f) => ({ ...f, seed: { ...f.seed, entities: replaceAt(f.seed.entities, i, patch) } }),
      now,
    );
  const setFact = (i: number, patch: Partial<KnownFact>, now = false) =>
    update((f) => ({ ...f, knowledge: replaceAt(f.knowledge, i, patch) }), now);
  const removeFact = (i: number) => {
    setEditing(new Set());
    update((f) => ({ ...f, knowledge: removeAt(f.knowledge, i) }), true);
  };
  const removeEntity = (i: number) => {
    setNameErrors({});
    update((f) => removeEntry(f, i), true);
  };

  /** The Player types a name: only the field changes. */
  const type = (row: Row, text: string) => {
    if (locked || guard.held()) return;
    setForm((f) => typeName(f, row, text));
    setNameErrors((e) => ({ ...e, [rowPath(row)]: undefined }));
  };
  const cancelName = (row: Row) => {
    setForm((f) => dropTyped(f, row));
    setNameErrors((e) => ({ ...e, [rowPath(row)]: undefined }));
  };
  /** Enter or blur: a rename through the core, which follows every reference. */
  const commitName = async (row: Row) => {
    if (guard.held() || locked) return;
    const change = nameChange(form, row);
    if (change.kind === "none") return cancelName(row);
    if (change.kind === "save") {
      const named = dropTyped(form, row);
      return update(() =>
        row === "pc"
          ? { ...named, seed: { ...named.seed, pc: { ...named.seed.pc, name: change.to } } }
          : { ...named, seed: { ...named.seed, entities: replaceAt(named.seed.entities, row, { name: change.to }) } },
      true);
    }
    const failure = await core(
      (flush) => afterFlush(flush, () => client.renameDraftEntity(draftId, change.entry, change.to)),
      (result) => setForm((f) => renamed(f, result, row)),
    );
    setNameErrors((e) => ({ ...e, [rowPath(row)]: failure ?? undefined }));
  };

  const confirm = async (row: Row) => {
    const failure = await core((flush) =>
      confirmRow(flush, (entry) => client.confirmAge(draftId, entry), form, row),
    );
    setError(failure);
  };

  /** Leaves only once the store holds the last edit; a failed save stays on screen. */
  const back = async () => {
    setLeaving(true);
    try {
      await saver.flush();
      onBack();
    } catch (e) {
      setError(`Could not save: ${errorText(e)}`);
      setLeaving(false);
    }
  };

  const approve = async () => {
    setApproving(true);
    setError(null);
    try {
      const result = await client.approveDraft(draftId);
      if (result.ok) onApproved(result.draft);
      else setGaps(result.gaps);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setApproving(false);
    }
  };

  const show = (at: string) => {
    const target = focusTarget(at);
    const el = document.getElementById(fieldId(target));
    el?.scrollIntoView({ block: "center" });
    el?.focus();
  };

  const toFix = (path: string) => <FieldGaps gaps={gapsAt(gaps, path)} form={form} />;
  const cutsOf = (id: SectionId) => cut.filter((c) => cutSections(c, form).includes(id));
  const countIn = (id: SectionId) => gaps.filter((g) => sectionOf(g.at, form) === id).length;

  /** A name field that renames through the core. */
  const name = (row: Row, label: string) => (
    <NameInput
      path={`${rowPath(row)}.name`}
      label={label}
      value={shownName(form, row)}
      saved={savedName(form, row)}
      error={nameErrors[rowPath(row)]}
      onType={(text) => type(row, text)}
      onCommit={() => void commitName(row)}
      onCancel={() => cancelName(row)}
    />
  );

  /**
   * An age, where it comes from, its "under 18" mark, and its "Confirm"
   * button until the core holds the Player's confirmation of this very age.
   */
  const age = (row: Row, value: number | null, onChange: (n: number | null) => void) => {
    const path = `${rowPath(row)}.age`;
    const saved = savedName(form, row);
    const line = ageLineOf(ages, form, row);
    const who = row === "pc" ? "your character" : saved || "this character";
    const label = ageLabel(line);
    const shown = ageStatus({ text: ageTexts[path] ?? null, value, line, pending, saveFailed, busy });
    return (
      <div className="review-age">
        <label htmlFor={fieldId(path)}>Age</label>
        <div className="review-age__row">
          <AgeInput
            path={path}
            label={`Age of ${who}`}
            value={value}
            nullable={row !== "pc"}
            onChange={onChange}
            onText={(text) => setAgeTexts((t) => ({ ...t, [path]: text }))}
          />
          <span className={`review-tag review-tag--${line?.source ?? "edited"}`}>{label}</span>
          {value !== null && value < 18 && <span className="review-tag review-tag--warn">under 18</span>}
          <span className="review-age__end">
            {shown.confirmed ? (
              <span className="review-tag review-tag--ok">
                <span aria-hidden="true">✓ </span>confirmed
              </span>
            ) : (
              <button
                type="button"
                className="play-btn play-btn--outline review-age__confirm"
                disabled={!shown.canConfirm}
                onClick={() => void confirm(row)}
              >
                Confirm age
              </button>
            )}
          </span>
        </div>
        {toFix(path)}
      </div>
    );
  };

  const kindSelect = (e: SeedEntity, i: number, kinds: [EntityKind, string][]) => (
    <div className="review-kind">
      <label htmlFor={fieldId(`entities[${i}].kind`)}>Kind</label>
      <select
        id={fieldId(`entities[${i}].kind`)}
        value={e.kind}
        onChange={(ev) => {
          const kind = ev.target.value as EntityKind;
          setEntity(i, { kind, age: isCharacter(kind) ? e.age : null }, true);
        }}
      >
        {e.kind === "pc" && <option value="pc">your character (not allowed here)</option>}
        {(kinds.some(([k]) => k === e.kind) || e.kind === "pc" ? kinds : KINDS).map(([k, label]) => (
          <option key={k} value={k}>
            {label}
          </option>
        ))}
      </select>
      {toFix(`entities[${i}].kind`)}
    </div>
  );

  /** One fact: its sentence and "edit"; edit mode or debug mode shows its three fields. */
  const fact = (k: KnownFact, i: number) => {
    const open = editing.has(i);
    const toggle = () =>
      setEditing((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      });
    return (
      <li key={i} className={`review-fact${open ? " review-fact--open" : ""}`}>
        <div className="review-fact__line">
          <span className={`review-fact__mark${k.open ? " review-fact__mark--open" : ""}`} aria-hidden="true" />
          <p className="review-fact__text">{k.text.trim() || <span className="review-hint">No sentence yet.</span>}</p>
          <button
            type="button"
            id={fieldId(`knowledge[${i}]`)}
            className="play-btn play-btn--quiet"
            aria-expanded={open}
            onClick={toggle}
          >
            {open ? "Done" : "edit"}
          </button>
          <button
            type="button"
            className="play-btn play-btn--text"
            aria-label={`Delete the fact: ${k.text}`}
            onClick={() => removeFact(i)}
          >
            Delete
          </button>
        </div>
        {toFix(`knowledge[${i}]`)}
        {open ? (
          <div className="review-fact__edit">
            <TextInput
              path={`knowledge[${i}].text`}
              label="The sentence, as your character knows it"
              rows={2}
              value={k.text}
              onChange={(text) => setFact(i, { text })}
            />
            <div className="review-fact__fields">
              {FACT_FIELDS.map(([field, label, hint]) => (
                <div key={field}>
                  <TextInput
                    path={`knowledge[${i}].${field}`}
                    label={label}
                    value={k[field]}
                    onChange={(text) => setFact(i, { [field]: text })}
                  />
                  <p className="review-hint">{hint}</p>
                </div>
              ))}
            </div>
            <label className="review-check">
              <input type="checkbox" checked={k.open} onChange={(ev) => setFact(i, { open: ev.target.checked }, true)} />
              Not done yet: it is planned or pending (an open matter)
            </label>
          </div>
        ) : (
          debug && (
            <dl className="review-fact__debug" aria-label="Internal fields, debug mode">
              {FACT_FIELDS.map(([field, label]) => (
                <div key={field}>
                  <dt>{label}</dt>
                  <dd>{k[field] || "—"}</dd>
                </div>
              ))}
            </dl>
          )
        )}
      </li>
    );
  };

  const indexed = seed.entities.map((e, i) => ({ e, i }));
  const people = indexed.filter(({ e }) => isCharacter(e.kind));
  const places = indexed.filter(({ e }) => !isCharacter(e.kind));
  const facts = form.knowledge.map((k, i) => ({ k, i }));
  const settled = facts.filter(({ k }) => !k.open);
  const openFacts = facts.filter(({ k }) => k.open);
  const details = seed.open_details;
  const placeNames = places.filter(({ e }) => e.kind === "place").map(({ e }) => e.name);
  const left = agesLeft(ages);
  const others = gaps.filter((g) => g.kind !== "age_unconfirmed").length;
  const ageRows: [Row, number | null][] = [
    ["pc", seed.pc.age],
    ...people.map(({ e, i }): [Row, number | null] => [i, e.age]),
  ];
  const agesShown = ageRows.every(([row, value]) =>
    ageTextMatches(ageTexts[`${rowPath(row)}.age`] ?? null, value),
  );
  const canApprove = approvable({ pending, busy, approving, saveFailed, error, gaps, state, agesShown });

  const add = (entry: SeedEntity) => {
    update((f) => addEntry(f, entry), true);
    setAdding(null);
  };

  const sectionBody: Record<SectionId, ReactNode> = {
    story: (
      <>
        {toFix("seed")}
        <TextInput path="premise" label="Premise" rows={3} value={seed.premise} onChange={(premise) => setSeed({ premise })} />
        {toFix("premise")}
        <TextInput
          path="story_language"
          label="Story language"
          value={seed.story_language}
          onChange={(story_language) => setSeed({ story_language })}
        />
        {toFix("story_language")}
        <h3 className="review-sub">Where it starts</h3>
        <TextInput path="start.place" label="Place" value={seed.start.place} onChange={(place) => setStart({ place })} />
        <p className="review-hint">
          {placeNames.length > 0 ? `A place of the draft: ${placeNames.join(", ")}.` : "Add a place first."}
        </p>
        {toFix("start.place")}
        <ListInput
          path="start.present"
          label="Who is present (comma between names)"
          separator=","
          value={seed.start.present}
          onChange={(present) => setStart({ present })}
        />
        {toFix("start.present")}
        <TextInput
          path="start.situation"
          label="Situation"
          rows={3}
          value={seed.start.situation}
          onChange={(situation) => setStart({ situation })}
        />
        <TextInput path="start.when" label="When" value={seed.start.when} onChange={(when) => setStart({ when })} />
      </>
    ),
    you: (
      <div className="review-entry review-entry--pc">
        {name("pc", "Name")}
        {toFix("pc.name")}
        {age("pc", seed.pc.age, (n) => n !== null && setPc({ age: n }))}
        <TextInput path="pc.concept" label="Who your character is" rows={2} value={seed.pc.concept} onChange={(concept) => setPc({ concept })} />
        {toFix("pc.concept")}
        <OtherNames
          path="pc.other_names"
          owner="your character"
          value={seed.pc.other_names}
          onChange={(other_names, now) => setPc({ other_names }, now)}
        />
        {toFix("pc.other_names")}
      </div>
    ),
    people: (
      <>
        {people.length === 0 && <p className="review-empty">No other person in the draft.</p>}
        <ul className="review-entries">
          {people.map(({ e, i }) => (
            <li key={i} className="review-entry">
              <div className="review-entry__head">
                {name(i, "Name")}
                {kindSelect(e, i, KINDS)}
                <button type="button" className="play-btn play-btn--text review-entry__delete" onClick={() => removeEntity(i)}>
                  Delete
                </button>
              </div>
              {toFix(`entities[${i}].name`)}
              {age(i, e.age, (n) => setEntity(i, { age: n }))}
              <OtherNames
                path={`entities[${i}].other_names`}
                owner={e.name || "this person"}
                value={e.other_names}
                onChange={(other_names, now) => setEntity(i, { other_names }, now)}
              />
              {toFix(`entities[${i}].other_names`)}
            </li>
          ))}
        </ul>
        {adding === "character" ? (
          <AddEntry kind="character" onAdd={add} onCancel={() => setAdding(null)} />
        ) : (
          <button type="button" className="play-btn play-btn--outline review-add" disabled={locked} onClick={() => setAdding("character")}>
            + Add a character
          </button>
        )}
      </>
    ),
    places: (
      <>
        {places.length === 0 && <p className="review-empty">No place or group in the draft.</p>}
        <ul className="review-entries">
          {places.map(({ e, i }) => (
            <li key={i} className="review-entry">
              <div className="review-entry__head">
                {name(i, "Name")}
                {kindSelect(e, i, PLACE_KINDS)}
                <button type="button" className="play-btn play-btn--text review-entry__delete" onClick={() => removeEntity(i)}>
                  Delete
                </button>
              </div>
              {toFix(`entities[${i}].name`)}
              {e.age !== null && age(i, e.age, (n) => setEntity(i, { age: n }))}
              <OtherNames
                path={`entities[${i}].other_names`}
                owner={e.name || "this entry"}
                value={e.other_names}
                onChange={(other_names, now) => setEntity(i, { other_names }, now)}
              />
              {toFix(`entities[${i}].other_names`)}
            </li>
          ))}
        </ul>
        {adding === "place" ? (
          <AddEntry kind="place" onAdd={add} onCancel={() => setAdding(null)} />
        ) : (
          <button type="button" className="play-btn play-btn--outline review-add" disabled={locked} onClick={() => setAdding("place")}>
            + Add a place
          </button>
        )}
      </>
    ),
    knows: (
      <>
        <p className="review-lead">These go into the Journal as what your character knew before the Campaign.</p>
        {settled.length === 0 ? (
          <p className="review-empty">No fact.</p>
        ) : (
          <ul className="review-facts">{settled.map(({ k, i }) => fact(k, i))}</ul>
        )}
      </>
    ),
    open: (
      <>
        <p className="review-lead">Planned or pending: not done yet. They stay open in play until something settles them.</p>
        {openFacts.length === 0 ? (
          <p className="review-empty">No open matter.</p>
        ) : (
          <ul className="review-facts">{openFacts.map(({ k, i }) => fact(k, i))}</ul>
        )}
        {details.length > 0 && (
          <>
            <h3 className="review-sub">Left open by your document</h3>
            <ul className="review-details">
              {details.map((d, i) => (
                <li key={i} className="review-detail">
                  <TextInput
                    path={`open_details[${i}].text`}
                    label={`Open detail ${i + 1}`}
                    hidden
                    value={d.text}
                    onChange={(text) => setSeed({ open_details: replaceAt(details, i, { text }) })}
                  />
                  <select
                    aria-label={`Who settles open detail ${i + 1}`}
                    value={d.mode}
                    onChange={(ev) => {
                      const mode = ev.target.value as OpenMode;
                      setSeed({ open_details: replaceAt(details, i, { mode }) }, true);
                    }}
                  >
                    {MODES.map(([m, label]) => (
                      <option key={m} value={m}>
                        {label}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="play-btn play-btn--text"
                    onClick={() => setSeed({ open_details: removeAt(details, i) }, true)}
                  >
                    Delete
                  </button>
                </li>
              ))}
            </ul>
            {toFix("open_details")}
          </>
        )}
      </>
    ),
    play: (
      <>
        <ListInput
          path="themes"
          label="Themes (comma between them)"
          separator=","
          value={seed.themes}
          onChange={(themes) => setSeed({ themes })}
        />
        <ListInput
          path="boundaries"
          label="Content limits (one per line)"
          separator={"\n"}
          value={seed.boundaries}
          onChange={(boundaries) => setSeed({ boundaries })}
        />
        <TextInput
          path="gm_agreement"
          label="How the GM runs the game"
          rows={3}
          value={seed.gm_agreement}
          onChange={(gm_agreement) => setSeed({ gm_agreement })}
        />
        <TextInput
          path="resolution"
          label="How actions are resolved"
          rows={2}
          value={seed.resolution}
          onChange={(resolution) => setSeed({ resolution })}
        />
        <label htmlFor={fieldId("min_age")}>Minimum age of any character</label>
        <AgeInput path="min_age" label="Minimum age of any character" value={seed.min_age} nullable onChange={(min_age) => setSeed({ min_age })} />
        {toFix("min_age")}
      </>
    ),
  };

  const status = pending
    ? "Saving…"
    : busy
      ? "Working…"
      : gaps.length === 0
        ? "Everything is checked. Approve to prepare the world."
        : [left > 0 ? `${left} ${left === 1 ? "age" : "ages"} to confirm` : null, others > 0 ? `${others} more to fix` : null]
            .filter(Boolean)
            .join(" · ");

  return (
    <div className="review">
      <nav className="review-nav" aria-label="Parts of the draft">
        <ol>
          {SECTIONS.map(([id, title], n) => {
            const count = countIn(id);
            return (
              <li key={id}>
                <a className="review-nav__item" href={`#review-${id}`}>
                  <span className="review-nav__num">{n + 1}</span>
                  <span className="review-nav__title">{title}</span>
                  {count > 0 ? (
                    <span className="review-nav__count" aria-label={`${count} to fix`}>
                      {count}
                    </span>
                  ) : (
                    <span className="review-nav__done" aria-label="nothing to fix">
                      ✓
                    </span>
                  )}
                </a>
              </li>
            );
          })}
        </ol>
      </nav>

      <div className="review-main">
        <header className="review-intro">
          <button type="button" className="play-btn play-btn--text review-intro__back" disabled={locked} onClick={() => void back()}>
            ‹ {leaving ? "Saving…" : "Saved drafts"}
          </button>
          <h2 className="review-intro__title">Check what the app read</h2>
          <p className="review-lead">
            Go through each part. Fix what is wrong, confirm each age, then approve. Every change is saved.
          </p>
          {state === "built" && (
            <p className="review-notice">A campaign was already built from this draft. Change something to build a new one.</p>
          )}
          <Progress ages={ages} others={others} />
          <NextGap gaps={gaps} form={form} onShow={show} />
          {cut.length > 0 && (
            <ul className="review-cuts">
              {cut.map((c) => (
                <li key={c.at} className="review-cut">
                  {cutSentence(c, form)}
                </li>
              ))}
            </ul>
          )}
        </header>

        {/* Locked while approval runs: a late edit would undo the approval. */}
        <fieldset className="plain" disabled={locked}>
          {SECTIONS.map(([id, title], n) => {
            const count = countIn(id);
            return (
              <section key={id} id={`review-${id}`} className="review-section" aria-labelledby={`review-${id}-title`}>
                <header className="review-section__head">
                  <span className="review-section__num">{n + 1}</span>
                  <h2 id={`review-${id}-title`} className="review-section__title">
                    {title}
                  </h2>
                  <span className={count > 0 ? "review-section__count" : "review-section__done"}>
                    {count > 0 ? `${count} to fix` : "Checked"}
                  </span>
                </header>
                {cutsOf(id).map((c) => (
                  <p key={c.at} className="review-cut">
                    {cutSentence(c, form)}
                  </p>
                ))}
                {sectionBody[id]}
              </section>
            );
          })}
        </fieldset>

        <footer className="review-bar">
          <p className="review-bar__status" role="status">
            {status}
          </p>
          {error && <p className="review-bar__error">{error}</p>}
          <button type="button" className="play-btn play-btn--primary" disabled={!canApprove} onClick={() => void approve()}>
            {approving ? "Approving…" : "Approve the draft"}
          </button>
        </footer>
      </div>
    </div>
  );
}

/** A flag for event handlers: one core action at a time, with no wait for a render. */
function createGuard() {
  let held = false;
  return {
    held: () => held,
    /** Takes the flag; false when another action holds it. */
    enter: () => !held && (held = true),
    leave: () => {
      held = false;
    },
  };
}

/** How far the check is: the ages the Player confirmed, and what is left to fix. */
function Progress({ ages, others }: { ages: AgeLine[]; others: number }) {
  const done = ages.length - agesLeft(ages);
  const share = ages.length === 0 ? 1 : done / ages.length;
  return (
    <div className="review-progress">
      <div className="review-progress__item">
        <p className="play-label">Ages confirmed</p>
        <p className="review-progress__value">
          {done} <span>of {ages.length}</span>
        </p>
        <div
          className="review-meter"
          role="progressbar"
          aria-label="Ages confirmed"
          aria-valuemin={0}
          aria-valuemax={ages.length}
          aria-valuenow={done}
        >
          <span style={{ width: `${Math.round(share * 100)}%` }} />
        </div>
      </div>
      <div className="review-progress__item">
        <p className="play-label">Other things to fix</p>
        <p className={`review-progress__value${others === 0 ? " review-progress__value--ok" : ""}`}>
          {others === 0 ? "None" : others}
        </p>
      </div>
    </div>
  );
}

/** The next gap in plain words, with a button that shows its field. */
function NextGap({ gaps, form, onShow }: { gaps: Gap[]; form: ReviewForm; onShow: (at: string) => void }) {
  const [index, setIndex] = useState(0);
  const i = Math.min(index, gaps.length - 1);
  const gap = gaps[i];
  if (!gap) return <p className="review-ready">Nothing is left to fix.</p>;
  return (
    <div className="review-next" role="status">
      <p className="play-label">
        To fix · {i + 1} of {gaps.length}
      </p>
      <p className="review-next__text">{gapSentence(gap, form)}</p>
      <div className="review-next__tools">
        <button type="button" className="play-btn play-btn--outline" onClick={() => onShow(gap.at)}>
          Show me
        </button>
        <button type="button" className="play-btn play-btn--text" disabled={i === 0} onClick={() => setIndex(i - 1)}>
          Previous
        </button>
        <button type="button" className="play-btn play-btn--text" disabled={i === gaps.length - 1} onClick={() => setIndex(i + 1)}>
          Next
        </button>
      </div>
    </div>
  );
}

/** Each gap of one field, next to it: what is wrong, then what to do. */
function FieldGaps({ gaps, form }: { gaps: Gap[]; form: ReviewForm }) {
  if (gaps.length === 0) return null;
  return (
    <>
      {gaps.map((g) => (
        <p key={`${g.kind}-${g.at}`} className="review-gap">
          {gapSentence(g, form)}
        </p>
      ))}
    </>
  );
}

/** The form of "Add a character" or "Add a place": a name first, so the entry has one from the start. */
function AddEntry({
  kind,
  onAdd,
  onCancel,
}: {
  kind: "character" | "place";
  onAdd: (entry: SeedEntity) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [entityKind, setEntityKind] = useState<EntityKind>(kind === "character" ? "npc" : "place");
  const [age, setAge] = useState<number | null>(null);
  const [missing, setMissing] = useState(false);
  const id = `add-${kind}`;
  const submit = () => {
    if (name.trim() === "") {
      setMissing(true);
      return;
    }
    onAdd({
      name: name.trim(),
      kind: entityKind,
      age: kind === "character" ? age : null,
      other_names: [],
      hidden: false,
    });
  };
  return (
    <div className="review-add-form" role="group" aria-labelledby={`${id}-title`}>
      <p id={`${id}-title`} className="play-label">
        {kind === "character" ? "Add a character the draft missed" : "Add a place or group the draft missed"}
      </p>
      <div className="review-add-form__fields">
        <div>
          <label htmlFor={`${id}-name`}>Name</label>
          <input
            id={`${id}-name`}
            value={name}
            autoFocus
            aria-invalid={missing || undefined}
            onChange={(e) => {
              setName(e.target.value);
              setMissing(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
              if (e.key === "Escape") onCancel();
            }}
          />
        </div>
        {kind === "character" ? (
          <div>
            <label htmlFor={fieldId(`${id}-age`)}>Age</label>
            <AgeInput path={`${id}-age`} label="Age" value={age} nullable onChange={setAge} />
          </div>
        ) : (
          <div>
            <label htmlFor={`${id}-kind`}>Kind</label>
            <select id={`${id}-kind`} value={entityKind} onChange={(e) => setEntityKind(e.target.value as EntityKind)}>
              {PLACE_KINDS.map(([k, label]) => (
                <option key={k} value={k}>
                  {label}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>
      {missing && <p className="review-gap">The name is empty. Write a name first.</p>}
      {kind === "character" && (
        <p className="review-hint">You confirm the age after you add the character, as for the others.</p>
      )}
      <div className="review-add-form__tools">
        <button type="button" className="play-btn play-btn--primary" onClick={submit}>
          Add
        </button>
        <button type="button" className="play-btn play-btn--text" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
