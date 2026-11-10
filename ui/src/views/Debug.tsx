import { useEffect, useState } from "react";
import { errorText, type DebugLine, type DebugView, type DraftLine, type StoryTurn } from "../api";
import { api } from "../tauri";

type Part = "turns" | "build";

/** Prompts, raw model output and timings. Debug mode only. Turn ids come from `PlayView.story`. */
export default function Debug({ turns }: { turns: StoryTurn[] }) {
  const [part, setPart] = useState<Part>("turns");
  return (
    <section>
      <div className="row">
        <button aria-pressed={part === "turns"} onClick={() => setPart("turns")}>
          Turns
        </button>
        <button aria-pressed={part === "build"} onClick={() => setPart("build")}>
          World Build
        </button>
      </div>
      {part === "turns" ? <TurnDebug turns={turns} /> : <BuildDebug />}
    </section>
  );
}

function TurnDebug({ turns }: { turns: StoryTurn[] }) {
  const [turnId, setTurnId] = useState<number | null>(turns.at(-1)?.turn_id ?? null);
  const [view, setView] = useState<DebugView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (turnId === null) return;
    api.getDebug(turnId).then(
      (v) => {
        setView(v);
        setError(null);
      },
      (e: unknown) => setError(errorText(e)),
    );
  }, [turnId]);

  if (turns.length === 0) return <p className="muted">No Turn yet.</p>;
  return (
    <>
      <label htmlFor="turn">Turn</label>
      <select
        id="turn"
        value={turnId ?? ""}
        onChange={(e) => setTurnId(Number(e.target.value))}
      >
        {turns.map((l) => (
          <option key={l.turn_id} value={l.turn_id}>
            {l.turn_id}: {l.kind === "opening" ? "(opening)" : l.intent.slice(0, 60)}
          </option>
        ))}
      </select>
      {error && <p className="error">{error}</p>}
      {view && (
        <>
          <p className="muted">
            op_id {view.op_id}, kind {view.kind}
          </p>
          <h2>Stored proposal</h2>
          <pre>{view.proposal_json}</pre>
          <h2>Steps</h2>
          <Steps entries={view.entries} />
        </>
      )}
    </>
  );
}

/**
 * The draft and World Build records of one draft. They reveal the hidden
 * world, so nothing loads before a second, explicit click.
 */
function BuildDebug() {
  const [drafts, setDrafts] = useState<DraftLine[]>([]);
  const [draftId, setDraftId] = useState("");
  const [entries, setEntries] = useState<DebugLine[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listDrafts().then(
      (all) => {
        setDrafts(all);
        setDraftId((id) => id || (all[0]?.draft_id ?? ""));
      },
      (e: unknown) => setError(errorText(e)),
    );
  }, []);

  const reveal = async () => {
    setError(null);
    try {
      setEntries(await api.getBuildDebug(draftId));
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (drafts.length === 0) {
    return error ? <p className="error">{error}</p> : <p className="muted">No draft yet.</p>;
  }
  return (
    <>
      <label htmlFor="draft">Draft</label>
      <select
        id="draft"
        value={draftId}
        onChange={(e) => {
          setDraftId(e.target.value);
          setEntries(null);
        }}
      >
        {drafts.map((d) => (
          <option key={d.draft_id} value={d.draft_id}>
            {d.title || d.draft_id} ({d.state})
          </option>
        ))}
      </select>
      {entries === null ? (
        <>
          <p className="error">
            These records show the hidden world: secrets, goals and future events. Reading them
            spoils the Campaign.
          </p>
          <div className="row">
            <button disabled={!draftId} onClick={() => void reveal()}>
              Show the secrets
            </button>
          </div>
        </>
      ) : entries.length === 0 ? (
        <p className="muted">No record for this draft.</p>
      ) : (
        <Steps entries={entries} />
      )}
      {error && <p className="error">{error}</p>}
    </>
  );
}

function Steps({ entries }: { entries: DebugLine[] }) {
  return (
    <>
      {entries.map((e, i) => (
        <details key={i}>
          <summary>
            {e.step} ({e.ms} ms)
          </summary>
          {e.prompt && (
            <>
              <label>Prompt</label>
              <pre>{e.prompt}</pre>
            </>
          )}
          <label>Output</label>
          <pre>{e.raw}</pre>
        </details>
      ))}
    </>
  );
}
