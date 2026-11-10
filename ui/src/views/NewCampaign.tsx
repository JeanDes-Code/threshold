import { useCallback, useEffect, useState } from "react";
import {
  errorText,
  parseBuildProgress,
  type BuildProgress,
  type CampaignSummary,
  type DraftLine,
  type DraftState,
  type DraftView,
} from "../api";
import { CANCEL_NOTE, failureText, progressText, readDocument } from "../newCampaign";
import { api, useEvent } from "../tauri";
import SeedReview from "./SeedReview";

interface Props {
  /** The World Build ended and its Campaign is open: "Begin" is next. */
  onOpened: (summary: CampaignSummary) => void;
  /** True while a Turn runs: no Campaign may open then. */
  locked: boolean;
}

/** Source: the text and the saved drafts. Review: the Seed Draft. Build: the World Build. */
type Screen =
  | { kind: "source" }
  | { kind: "review"; view: DraftView }
  | { kind: "build"; view: DraftView };

const STATES: Record<DraftState, string> = {
  draft: "to check",
  approved: "approved, not built",
  built: "built",
};

const screenOf = (view: DraftView): Screen =>
  view.state === "approved" ? { kind: "build", view } : { kind: "review", view };

/** A new Campaign from a document: Source Text, review, World Build. */
export default function NewCampaign({ onOpened, locked }: Props) {
  const [screen, setScreen] = useState<Screen>({ kind: "source" });
  const [text, setText] = useState("");
  const [drafts, setDrafts] = useState<DraftLine[]>([]);
  const [job, setJob] = useState<"draft" | "build" | null>(null);
  const [progress, setProgress] = useState<BuildProgress | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.listDrafts().then(setDrafts, (e: unknown) =>
      setError(`Could not list the drafts: ${errorText(e)}`),
    );
  }, []);
  useEffect(refresh, [refresh]);

  useEvent<unknown>("build_progress", (payload) => {
    const p = parseBuildProgress(payload);
    if (p && job) setProgress(p);
  });

  const go = (next: Screen) => {
    setError(null);
    setToDelete(null);
    setScreen(next);
  };

  /** Runs a draft or a build. A finished job after "Cancel" still counts. */
  const run = async (kind: "draft" | "build", call: () => Promise<void>) => {
    setJob(kind);
    setProgress(null);
    setCancelling(false);
    setError(null);
    try {
      await call();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setJob(null);
      refresh();
    }
  };

  const draft = () =>
    run("draft", async () => {
      const view = await api.draftFromText(text);
      setText("");
      go({ kind: "review", view });
    });

  const build = (view: DraftView) =>
    run("build", async () => {
      const summary = await api.startBuild(view.draft_id);
      go({ kind: "source" });
      onOpened(summary);
    });

  const cancel = async () => {
    setCancelling(true);
    try {
      await api.cancelBuild();
    } catch (e) {
      setError(`Could not cancel: ${errorText(e)}`);
    }
  };

  /** Opens a saved draft where it stands; `review` opens its review in any state. */
  const open = async (draftId: string, review = false) => {
    try {
      const view = await api.getDraft(draftId);
      go(review ? { kind: "review", view } : screenOf(view));
    } catch (e) {
      setError(`Could not open the draft: ${errorText(e)}`);
    }
  };

  const remove = async (draftId: string) => {
    try {
      await api.deleteDraft(draftId);
      setToDelete(null);
    } catch (e) {
      setError(`Could not delete the draft: ${errorText(e)}`);
    }
    refresh();
  };

  /** The size is checked before the file is read: a huge file never loads. */
  const loadFile = async (file: File) => {
    try {
      const read = await readDocument(file);
      if (read.ok) {
        setText(read.text);
        setError(null);
      } else {
        setError(read.message);
      }
    } catch (e) {
      setError(`Could not read the file: ${errorText(e)}`);
    }
  };

  const running = (
    <div className="row">
      <span className="progress">{progress ? progressText(progress) : "Starting…"}</span>
      <button disabled={cancelling} onClick={() => void cancel()}>
        {cancelling ? "Cancelling…" : "Cancel"}
      </button>
      <span className="muted">{CANCEL_NOTE}</span>
    </div>
  );

  if (screen.kind === "review") {
    return (
      <SeedReview
        key={screen.view.draft_id}
        view={screen.view}
        onApproved={(view) => go({ kind: "build", view })}
        onBack={() => {
          go({ kind: "source" });
          refresh();
        }}
      />
    );
  }

  if (screen.kind === "build") {
    const { view } = screen;
    const failure = error ?? (view.last_failure ? failureText(view.last_failure) : null);
    return (
      <section>
        <h2>Prepare the world</h2>
        <p className="muted">
          The app now makes the hidden part of the world: what the other characters want and hide,
          and what will happen. You will not see it. This can take several minutes.
        </p>
        {job === "build" ? (
          running
        ) : (
          <>
            {failure && <p className="error">{failure}</p>}
            <div className="row">
              <button disabled={locked} onClick={() => void build(view)}>
                {failure ? "Retry" : "Prepare the world"}
              </button>
              <button onClick={() => void open(view.draft_id, true)}>
                Back to review
              </button>
            </div>
            {locked && <p className="muted">A Turn is running; wait for it to finish.</p>}
          </>
        )}
      </section>
    );
  }

  return (
    <section>
      <h2>New campaign from a document</h2>
      <p className="muted">
        Paste your Source Text, or choose a .md or .txt file. The app reads it and drafts the
        Adventure Seed. You check the draft before anything else happens.
      </p>
      <label htmlFor="source">Source Text</label>
      <textarea
        id="source"
        rows={10}
        value={text}
        disabled={job !== null}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="row">
        <input
          type="file"
          className="file"
          accept=".md,.txt,text/markdown,text/plain"
          aria-label="Choose a .md or .txt file"
          disabled={job !== null}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void loadFile(file);
            e.target.value = "";
          }}
        />
        <button disabled={job !== null || !text.trim()} onClick={() => void draft()}>
          Draft
        </button>
      </div>
      {job === "draft" ? running : <p className="muted">A draft takes a few minutes.</p>}
      {error && <p className="error">{error}</p>}

      <h2>Saved drafts</h2>
      {drafts.length === 0 ? (
        <p className="muted">No draft yet.</p>
      ) : (
        <ul className="list">
          {drafts.map((d) => (
            <li key={d.draft_id} className="row">
              <span className="grow">
                {d.title || "(no premise)"}
                <span className="mark">{STATES[d.state] ?? d.state}</span>
              </span>
              <button disabled={job !== null} onClick={() => void open(d.draft_id)}>
                Open
              </button>
              {toDelete === d.draft_id ? (
                <button disabled={job !== null} onClick={() => void remove(d.draft_id)}>
                  Delete for good
                </button>
              ) : (
                <button disabled={job !== null} onClick={() => setToDelete(d.draft_id)}>
                  Delete
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
