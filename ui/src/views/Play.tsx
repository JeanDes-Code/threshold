import { useRef, useState } from "react";
import {
  BEGIN_OP_ID,
  committedLine,
  errorText,
  rollText,
  type Attempt,
  type CampaignSummary,
  type NarrationChunk,
  type RollResult,
  type TurnLine,
} from "../api";
import { api, turns, useEvent } from "../tauri";

interface Props {
  campaign: CampaignSummary;
  lines: TurnLine[];
  onLine: (line: TurnLine) => void;
  onNarration: (turnId: number, narration: string) => void;
  /** Reloads every Turn line from the campaign (reverted marks included). */
  onRefresh: () => void;
  /** Told whenever a Turn starts or ends. */
  onBusy: (busy: boolean) => void;
}

export default function Play({ campaign, lines, onLine, onNarration, onRefresh, onBusy }: Props) {
  const [intent, setIntent] = useState("");
  const [busy, setBusyHere] = useState(false);
  const [stream, setStream] = useState("");
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [ooc, setOoc] = useState("");
  const [answer, setAnswer] = useState<string | null>(null);
  const [fixing, setFixing] = useState(false);
  const [clarified, setClarified] = useState("");
  const [why, setWhy] = useState("");
  // The op_id whose narration chunks this view shows.
  const streaming = useRef<string | null>(null);

  // App hears it directly, so it stays right even if this view unmounts
  // (a tab switch) while a Turn runs.
  const setBusy = (on: boolean) => {
    setBusyHere(on);
    onBusy(on);
  };

  useEvent<NarrationChunk>("narration_chunk", (chunk) => {
    if (chunk.op_id === streaming.current) setStream((s) => s + chunk.text);
  });

  const play = async (attempt: Attempt, kind: string, text: string) => {
    streaming.current = attempt.opId;
    setStream("");
    setFailure(null);
    setNotice(null);
    setBusy(true);
    try {
      const committed = committedLine(await attempt.result, kind, text);
      onLine(committed.line);
      setNotice(committed.notice);
      // A Correction reverts the Turn before it; a failed narration may
      // leave the view behind the store. Reload the lines in both cases.
      if (kind === "correction" || committed.notice) onRefresh();
      return true;
    } catch (e) {
      setFailure(errorText(e));
      return false;
    } finally {
      streaming.current = null;
      setStream("");
      setBusy(false);
    }
  };

  const submit = async () => {
    const text = intent.trim();
    if (!text) return;
    if (await play(turns.submit(text), "play", text)) setIntent("");
  };

  const retry = async () => {
    const pending = turns.pending();
    const attempt = turns.retry();
    if (!pending || !attempt) return;
    const text = String(pending.args.text ?? pending.args.clarified ?? "");
    const kind = pending.cmd === "correct_last" ? "correction" : "play";
    if ((await play(attempt, kind, text)) && kind === "play") setIntent("");
  };

  const correct = async () => {
    const text = clarified.trim();
    if (!text || !why.trim()) return;
    if (await play(turns.correct(text, why.trim()), "correction", text)) {
      setFixing(false);
      setClarified("");
      setWhy("");
    }
  };

  const renarrate = async (turnId: number) => {
    // The chunks come under the Turn's old op_id; the view shows the result.
    setBusy(true);
    setFailure(null);
    setNotice(null);
    try {
      onNarration(turnId, await api.renarrate(turnId));
    } catch (e) {
      // The Turn stays committed; only its narration failed again.
      setNotice(`Committed, narration interrupted: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const ask = async () => {
    const text = ooc.trim();
    if (!text) return;
    setBusy(true);
    try {
      setAnswer(await api.askOoc(text));
      setOoc("");
    } catch (e) {
      setAnswer(`OOC failed: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  // "Begin": the GM narrates the opening scene, as the Opening Turn.
  const begin = () => play({ opId: BEGIN_OP_ID, result: api.begin() }, "opening", "");

  const last = lines.at(-1);
  const needsBegin =
    campaign.needs_begin && !lines.some((l) => l.kind === "opening" || l.kind === "play");

  return (
    <section>
      <p className="muted">
        {campaign.pc_name}: {campaign.premise}
      </p>
      {lines.map((line) => (
        <article key={line.turn_id} className={line.reverted ? "turn reverted" : "turn"}>
          {/* An Opening Turn has no Intent: only its Narration shows. */}
          {line.kind !== "opening" && (
            <div className="intent">
              {line.kind === "correction" ? "Correction: " : "> "}
              {line.intent}
              {line.reverted && <span className="tag"> (reverted by a Correction)</span>}
            </div>
          )}
          {line.roll && <Roll roll={line.roll} />}
          {line.narration !== null ? (
            <div className="narration">{line.narration}</div>
          ) : (
            <button disabled={busy} onClick={() => void renarrate(line.turn_id)}>
              Narration missing: narrate again
            </button>
          )}
        </article>
      ))}
      {busy && (
        <article className="turn">
          <div className="narration">{stream || "…"}</div>
        </article>
      )}
      {notice && <div className="notice">{notice}</div>}
      {failure && (
        <div className="error">
          The Turn failed. Nothing was committed.
          {"\n"}
          {failure}
          {turns.pending() && (
            <div className="row">
              <button disabled={busy} onClick={() => void retry()}>
                Retry
              </button>
            </div>
          )}
        </div>
      )}

      {needsBegin ? (
        <div className="row">
          <button disabled={busy} onClick={() => void begin()}>
            Begin
          </button>
          <span className="muted">The GM sets the opening scene.</span>
        </div>
      ) : (
        <>
          <label htmlFor="intent">What does {campaign.pc_name} do?</label>
          <textarea
            id="intent"
            rows={3}
            value={intent}
            disabled={busy}
            onChange={(e) => setIntent(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void submit();
              }
            }}
          />
          <div className="row">
            <button disabled={busy || !intent.trim()} onClick={() => void submit()}>
              Play
            </button>
            <button disabled={busy || !last || last.kind !== "play"} onClick={() => setFixing((f) => !f)}>
              Correct last Turn…
            </button>
          </div>

          {fixing && (
            <div>
              <label htmlFor="clarified">What you meant</label>
              <input id="clarified" value={clarified} onChange={(e) => setClarified(e.target.value)} />
              <label htmlFor="why">Why the last Turn was wrong</label>
              <input id="why" value={why} onChange={(e) => setWhy(e.target.value)} />
              <div className="row">
                <button disabled={busy || !clarified.trim() || !why.trim()} onClick={() => void correct()}>
                  Apply Correction
                </button>
              </div>
            </div>
          )}
        </>
      )}

      <h2>Out of character</h2>
      <div className="row">
        <input
          aria-label="Out-of-character question"
          placeholder="Ask the GM a question outside the story"
          value={ooc}
          disabled={busy}
          onChange={(e) => setOoc(e.target.value)}
        />
        <button disabled={busy || !ooc.trim()} onClick={() => void ask()}>
          Ask
        </button>
      </div>
      {answer && <p className="ooc">{answer}</p>}
    </section>
  );
}

function Roll({ roll }: { roll: RollResult }) {
  return <div className={`roll ${roll.success ? "ok" : "bad"}`}>{rollText(roll)}</div>;
}
