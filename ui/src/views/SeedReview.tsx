import { useEffect, useState } from "react";
import {
  errorText,
  type AdventureSeed,
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
  agesToConfirm,
  confirmRow,
  createSaver,
  fieldId,
  flagsFromNames,
  focusTarget,
  gapSentence,
  isUnconfirmed,
  removeAt,
  removeEntityRow,
  replaceAt,
  type ReviewForm,
  type Row,
} from "../newCampaign";
import { api } from "../tauri";
import { AgeInput, ListInput, OtherNames, TextInput } from "./fields";

interface Props {
  view: DraftView;
  /** The draft is approved: the World Build may start. */
  onApproved: (view: DraftView) => void;
  onBack: () => void;
}

const KINDS: [EntityKind, string][] = [
  ["npc", "character"],
  ["place", "place"],
  ["faction", "group"],
  ["item", "object"],
];
const MODES: [OpenMode, string][] = [
  ["delegated", "the GM decides"],
  ["discover", "found in play"],
  ["to_agree", "agree before play"],
];
const FACT_FIELDS = [
  ["subject", "About"],
  ["key", "Key"],
  ["value", "Value"],
] as const;
const isCharacter = (kind: EntityKind) => kind === "npc" || kind === "pc";

/**
 * The Seed Draft review: every edit is saved (after a pause in typing) and
 * brings back the fresh gaps. "Approve" waits for no gap and no pending save.
 */
export default function SeedReview({ view, onApproved, onBack }: Props) {
  const [form, setForm] = useState<ReviewForm>({
    seed: view.seed,
    knowledge: view.knowledge,
    unconfirmed: flagsFromNames(view.seed, view.ages_to_confirm),
  });
  const [gaps, setGaps] = useState<Gap[]>(view.gaps);
  const [state, setState] = useState(view.state);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [approving, setApproving] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const draftId = view.draft_id;
  // One saver per mount: the parent gives each draft its own key.
  const [saver] = useState(() =>
    createSaver<ReviewForm, DraftView>({
      delayMs: 600,
      save: (f) => api.saveDraft(draftId, f.seed, f.knowledge, agesToConfirm(f)),
      onResult: (saved) => {
        setGaps(saved.gaps);
        setState(saved.state);
        setError(null);
      },
      onError: (e) => setError(`Could not save: ${errorText(e)}`),
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

  /** `now`: a click, saved at once; typing waits for a pause. */
  const update = (change: (f: ReviewForm) => ReviewForm, now = false) => {
    if (locked) return;
    const next = change(form);
    setForm(next);
    saver.edit(next, now);
  };
  const seed = form.seed;
  const setSeed = (patch: Partial<AdventureSeed>, now = false) =>
    update((f) => ({ ...f, seed: { ...f.seed, ...patch } }), now);
  const setStart = (patch: Partial<StartSeed>) => setSeed({ start: { ...seed.start, ...patch } });
  // The unconfirmed marks belong to rows, so a rename never moves one.
  const setPc = (patch: Partial<PcSeed>, now = false) =>
    update((f) => ({ ...f, seed: { ...f.seed, pc: { ...f.seed.pc, ...patch } } }), now);
  const setEntity = (i: number, patch: Partial<SeedEntity>, now = false) =>
    update(
      (f) => ({ ...f, seed: { ...f.seed, entities: replaceAt(f.seed.entities, i, patch) } }),
      now,
    );
  const removeEntity = (i: number) => update((f) => removeEntityRow(f, i), true);
  const setFact = (i: number, patch: Partial<KnownFact>, now = false) =>
    update((f) => ({ ...f, knowledge: replaceAt(f.knowledge, i, patch) }), now);
  const removeFact = (i: number) =>
    update((f) => ({ ...f, knowledge: removeAt(f.knowledge, i) }), true);
  const confirm = (row: Row) => update((f) => confirmRow(f, row), true);

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
      const result = await api.approveDraft(draftId);
      if (result.ok) onApproved(result.draft);
      else setGaps(result.gaps);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setApproving(false);
    }
  };

  /** An age input, its "under 18" mark, and its "Confirm" button while the age is a guess. */
  const age = (row: Row, value: number | null, onChange: (n: number | null) => void) => {
    const path = row === "pc" ? "pc.age" : `entities[${row}].age`;
    const name = row === "pc" ? seed.pc.name : (seed.entities[row]?.name ?? "");
    return (
      <span className="age">
        <AgeInput
          path={path}
          label={`Age of ${name || "this character"}`}
          value={value}
          nullable={row !== "pc"}
          onChange={onChange}
        />
        {value !== null && value < 18 && <span className="mark warn">under 18</span>}
        {isUnconfirmed(form, row) && (
          <>
            <span className="mark warn">proposed</span>
            <button onClick={() => confirm(row)}>Confirm</button>
          </>
        )}
      </span>
    );
  };

  const kindSelect = (e: SeedEntity, i: number) => (
    <select
      id={fieldId(`entities[${i}].kind`)}
      aria-label={`Kind of ${e.name}`}
      value={e.kind}
      onChange={(ev) => {
        const kind = ev.target.value as EntityKind;
        setEntity(i, { kind, age: isCharacter(kind) ? e.age : null }, true);
      }}
    >
      {e.kind === "pc" && <option value="pc">your character (not allowed here)</option>}
      {KINDS.map(([k, label]) => (
        <option key={k} value={k}>
          {label}
        </option>
      ))}
    </select>
  );

  const indexed = seed.entities.map((e, i) => ({ e, i }));
  const characters = indexed.filter(({ e }) => isCharacter(e.kind));
  const others = indexed.filter(({ e }) => !isCharacter(e.kind));
  const details = seed.open_details;
  const canApprove =
    !pending && !approving && gaps.length === 0 && error === null && state !== "built";

  return (
    <section>
      <div className="row">
        <button disabled={locked} onClick={() => void back()}>
          {leaving ? "Saving…" : "Back to the list"}
        </button>
      </div>
      <h2>Check the draft</h2>
      <p className="muted">
        This is what the app read in your document. Fix what is wrong. Every change is saved.
      </p>
      {state === "built" && (
        <p className="notice">
          A campaign was already built from this draft. Change something to build a new one.
        </p>
      )}
      <Gaps gaps={gaps} form={form} />

      {/* Locked while approval runs: a late edit would undo the approval. */}
      <fieldset className="plain" disabled={locked}>
        <h2>The story</h2>
        <TextInput
          path="premise"
          label="Premise"
          rows={3}
          value={seed.premise}
          onChange={(premise) => setSeed({ premise })}
        />
        <TextInput
          path="story_language"
          label="Story language"
          value={seed.story_language}
          onChange={(story_language) => setSeed({ story_language })}
        />

        <h2>Your character</h2>
        <TextInput
          path="pc.name"
          label="Name"
          value={seed.pc.name}
          onChange={(name) => setPc({ name })}
        />
        <label htmlFor={fieldId("pc.age")}>Age</label>
        {age("pc", seed.pc.age, (n) => n !== null && setPc({ age: n }))}
        <TextInput
          path="pc.concept"
          label="Concept"
          rows={2}
          value={seed.pc.concept}
          onChange={(concept) => setPc({ concept })}
        />
        <label>Other names</label>
        <OtherNames
          path="pc.other_names"
          value={seed.pc.other_names}
          onChange={(other_names, now) => setPc({ other_names }, now)}
        />

        <h2>How you want to play</h2>
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
        <AgeInput
          path="min_age"
          label="Minimum age of any character"
          value={seed.min_age}
          nullable
          onChange={(min_age) => setSeed({ min_age })}
        />

        <h2>Where it starts</h2>
        <TextInput
          path="start.place"
          label="Place"
          value={seed.start.place}
          onChange={(place) => setStart({ place })}
        />
        <ListInput
          path="start.present"
          label="Who is present (comma between names)"
          separator=","
          value={seed.start.present}
          onChange={(present) => setStart({ present })}
        />
        <TextInput
          path="start.situation"
          label="Situation"
          rows={3}
          value={seed.start.situation}
          onChange={(situation) => setStart({ situation })}
        />
        <TextInput
          path="start.when"
          label="When"
          value={seed.start.when}
          onChange={(when) => setStart({ when })}
        />

        {details.length > 0 && (
          <>
            <h2>Left open by the document</h2>
            <ul className="list">
              {details.map((d, i) => (
                <li key={i} className="row">
                  <TextInput
                    path={`open_details[${i}].text`}
                    label="Open detail"
                    hidden
                    value={d.text}
                    onChange={(text) => setSeed({ open_details: replaceAt(details, i, { text }) })}
                  />
                  <select
                    aria-label="Who settles it"
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
                  <button onClick={() => setSeed({ open_details: removeAt(details, i) }, true)}>
                    Delete
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}

        <h2>Characters</h2>
        {characters.length === 0 ? (
          <p className="muted">No other character.</p>
        ) : (
          <div className="scroll">
            <table className="roster">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Other names</th>
                  <th>Kind</th>
                  <th>Age</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {characters.map(({ e, i }) => (
                  <tr key={i}>
                    <td>
                      <TextInput
                        path={`entities[${i}].name`}
                        label="Name"
                        hidden
                        value={e.name}
                        onChange={(name) => setEntity(i, { name })}
                      />
                    </td>
                    <td>
                      <OtherNames
                        path={`entities[${i}].other_names`}
                        value={e.other_names}
                        onChange={(other_names, now) => setEntity(i, { other_names }, now)}
                      />
                    </td>
                    <td>{kindSelect(e, i)}</td>
                    <td>
                      {age(i, e.age, (n) => setEntity(i, { age: n }))}
                    </td>
                    <td>
                      <button onClick={() => removeEntity(i)}>Delete</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <h2>Places, groups and objects</h2>
        {others.length === 0 ? (
          <p className="muted">None.</p>
        ) : (
          <ul className="list">
            {others.map(({ e, i }) => (
              <li key={i}>
                <div className="row">
                  <TextInput
                    path={`entities[${i}].name`}
                    label="Name"
                    hidden
                    value={e.name}
                    onChange={(name) => setEntity(i, { name })}
                  />
                  {kindSelect(e, i)}
                  <button onClick={() => removeEntity(i)}>Delete</button>
                </div>
                {e.age !== null && (
                  <div className="row">
                    {age(i, e.age, (n) => setEntity(i, { age: n }))}
                    <span className="muted">Only a character has an age: clear it.</span>
                  </div>
                )}
                <OtherNames
                  path={`entities[${i}].other_names`}
                  value={e.other_names}
                  onChange={(other_names, now) => setEntity(i, { other_names }, now)}
                />
              </li>
            ))}
          </ul>
        )}

        <h2>Facts your character knows</h2>
        <p className="muted">
          These go into the Journal. An open fact is planned or pending: it has not happened yet.
        </p>
        {form.knowledge.length === 0 ? (
          <p className="muted">None.</p>
        ) : (
          <ul className="list">
            {form.knowledge.map((k, i) => (
              <li key={i}>
                <div className="row">
                  <label className="toggle" title="Planned or pending: not done yet">
                    <input
                      type="checkbox"
                      checked={k.open}
                      onChange={(ev) => setFact(i, { open: ev.target.checked }, true)}
                    />
                    open
                  </label>
                  <TextInput
                    path={`knowledge[${i}].text`}
                    label="Fact"
                    hidden
                    value={k.text}
                    onChange={(text) => setFact(i, { text })}
                  />
                  <button onClick={() => removeFact(i)}>Delete</button>
                </div>
                <div className="row small">
                  {FACT_FIELDS.map(([field, label]) => (
                    <TextInput
                      key={field}
                      path={`knowledge[${i}].${field}`}
                      label={label}
                      hidden
                      placeholder={label}
                      value={k[field]}
                      onChange={(text) => setFact(i, { [field]: text })}
                    />
                  ))}
                </div>
              </li>
            ))}
          </ul>
        )}
      </fieldset>

      <div className="row review-bar">
        <button disabled={!canApprove} onClick={() => void approve()}>
          Approve
        </button>
        <span className="muted">
          {pending
            ? "Saving…"
            : gaps.length > 0
              ? `${gaps.length} to fix before you approve.`
              : "Saved."}
        </span>
        {error && <span className="error">{error}</span>}
      </div>
    </section>
  );
}

/** The gaps, one at a time, with a button that shows the field. */
function Gaps({ gaps, form }: { gaps: Gap[]; form: ReviewForm }) {
  const [index, setIndex] = useState(0);
  const i = Math.min(index, gaps.length - 1);
  const gap = gaps[i];
  if (!gap) return <p className="ok">Nothing is missing.</p>;
  const show = () => {
    const el = document.getElementById(fieldId(focusTarget(gap.at)));
    el?.scrollIntoView({ block: "center" });
    el?.focus();
  };
  return (
    <div className="gap" role="status">
      <strong>
        To fix: {i + 1} of {gaps.length}
      </strong>
      <p>{gapSentence(gap, form)}</p>
      <div className="row">
        <button onClick={show}>Show me</button>
        <button disabled={i === 0} onClick={() => setIndex(i - 1)}>
          Previous
        </button>
        <button disabled={i === gaps.length - 1} onClick={() => setIndex(i + 1)}>
          Next
        </button>
      </div>
    </div>
  );
}
