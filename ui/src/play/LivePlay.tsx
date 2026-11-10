import { useRef, useState, type ReactNode } from "react";
import {
  BEGIN_OP_ID,
  errorText,
  turnNotice,
  type Attempt,
  type NarrationChunk,
  type PlayView,
  type Target,
} from "../api";
import { api, play, turns, useEvent } from "../tauri";
import PlayScreen from "./PlayScreen";

interface Props {
  view: PlayView;
  /** Takes the view the core gives after each Turn. */
  onView: (view: PlayView) => void;
  /** Told whenever a Turn starts or ends. */
  onBusy: (busy: boolean) => void;
  /** The Correction fields are open: the Campaign menu opens them. */
  fixing: boolean;
  onFixing: (open: boolean) => void;
  /** The screen switch and the Campaign menu. */
  tools: ReactNode;
}

/** Narration again: its chunks come under the Turn's old op_id, so any chunk counts. */
const ANY_OP = "*";

/**
 * The Play screen on a real Campaign. While a Turn runs, the story shows the
 * raw stream; when it ends, the core's new PlayView replaces it.
 */
export default function LivePlay({ view, onView, onBusy, fixing, onFixing, tools }: Props) {
  const [busy, setBusyHere] = useState(false);
  const [stream, setStream] = useState<string | null>(null);
  const [pendingIntent, setPendingIntent] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  // The op_id whose narration chunks the story shows.
  const streaming = useRef<string | null>(null);

  // App hears it directly, so it stays right even if this screen unmounts
  // (a tab switch) while a Turn runs.
  const setBusy = (on: boolean) => {
    setBusyHere(on);
    onBusy(on);
  };

  useEvent<NarrationChunk>("narration_chunk", (chunk) => {
    const op = streaming.current;
    if (op !== null && (op === ANY_OP || chunk.op_id === op)) setStream((s) => (s ?? "") + chunk.text);
  });

  /** One view call after the Turn ends, committed or failed. */
  const reload = async () => {
    try {
      onView(await play.turnEnded());
    } catch (e) {
      setNotice(`Could not reload the story: ${errorText(e)}`);
    }
  };

  /** Runs one Turn. Resolves true when it committed. */
  const run = async (attempt: Attempt, intent: string | null): Promise<boolean> => {
    streaming.current = attempt.opId;
    setStream("");
    setPendingIntent(intent);
    setFailure(null);
    setNotice(null);
    setBusy(true);
    let committed = false;
    try {
      setNotice(turnNotice(await attempt.result));
      committed = true;
    } catch (e) {
      setFailure(errorText(e));
    }
    streaming.current = null;
    // The new view lands before the stream goes, so the story does not jump back.
    await reload();
    setStream(null);
    setPendingIntent(null);
    setBusy(false);
    return committed;
  };

  const act = (text: string) => run(turns.submit(text), text);

  const correct = (clarified: string, why: string) => run(turns.correct(clarified, why), clarified);

  const retry = () => {
    const pending = turns.pending();
    const attempt = turns.retry();
    if (!pending || !attempt) return;
    void run(attempt, String(pending.args.text ?? pending.args.clarified ?? ""));
  };

  // "Begin": the GM narrates the opening scene, as the Opening Turn.
  const begin = () => void run({ opId: BEGIN_OP_ID, result: api.begin() }, null);

  const renarrate = async (turnId: number) => {
    streaming.current = ANY_OP;
    setStream("");
    setFailure(null);
    setNotice(null);
    setBusy(true);
    try {
      await api.renarrate(turnId);
    } catch (e) {
      // The Turn stays committed; only its narration failed again.
      setNotice(`Committed, narration interrupted: ${errorText(e)}`);
    }
    streaming.current = null;
    await reload();
    setStream(null);
    setBusy(false);
  };

  const ask = async (text: string): Promise<boolean> => {
    setBusy(true);
    try {
      setAnswer(await api.askOoc(text));
      return true;
    } catch (e) {
      setAnswer(`OOC failed: ${errorText(e)}`);
      return false;
    } finally {
      setBusy(false);
    }
  };

  /** One view call after each note command: the Pinned card and the cards change. */
  const marksChanged = async () => {
    try {
      onView(await play.noteChanged());
    } catch (e) {
      setNotice(`Could not reload the story: ${errorText(e)}`);
    }
  };

  const pin = async (target: Target, on: boolean) => {
    try {
      await api.setPin(target, on);
    } catch (e) {
      setNotice(`Could not change the pin: ${errorText(e)}`);
    }
    await marksChanged();
  };

  const note = async (body: string, about: Target | null): Promise<string | null> => {
    try {
      await api.addNote(body, about);
    } catch (e) {
      return errorText(e);
    }
    await marksChanged();
    return null;
  };

  return (
    <PlayScreen
      view={view}
      busy={busy}
      stream={stream}
      pendingIntent={pendingIntent}
      answer={answer}
      failure={failure}
      notice={notice}
      canRetry={turns.pending() !== null}
      loadCard={api.getEntityCard}
      onAct={act}
      onAsk={ask}
      onCorrect={correct}
      onRetry={retry}
      onBegin={begin}
      onRenarrate={(turnId) => void renarrate(turnId)}
      onDismissAnswer={() => setAnswer(null)}
      onPin={(target, on) => void pin(target, on)}
      onNote={note}
      fixing={fixing}
      onFixing={onFixing}
      tools={tools}
    />
  );
}
