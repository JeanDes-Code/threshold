import type { ReactNode } from "react";
import { BEGIN_OP_ID, errorText, renarrateOpId, type PlayView, type Target } from "../api";
import { api, play, turns } from "../tauri";
import PlayScreen from "./PlayScreen";
import type { TurnState, TurnStore } from "./session";

interface Props {
  view: PlayView;
  /** Takes the view the core gives after each note command. */
  onView: (view: PlayView) => void;
  /** The running Turn. App holds it, so a tab switch does not lose it. */
  turn: TurnState;
  store: TurnStore;
  /** The Correction fields are open: the Campaign menu opens them. */
  fixing: boolean;
  onFixing: (open: boolean) => void;
  /** The screen switch and the Campaign menu. */
  tools: ReactNode;
}

/**
 * The Play screen on a real Campaign. While a Turn runs, the story shows the
 * raw stream; when it ends, the core's new PlayView replaces it.
 */
export default function LivePlay({ view, onView, turn, store, fixing, onFixing, tools }: Props) {
  const act = (text: string) => store.run(() => turns.submit(text), text);

  const correct = (clarified: string, why: string) => store.run(() => turns.correct(clarified, why), clarified);

  const retry = async () => {
    const pending = turns.pending();
    if (!pending) return false;
    return store.run(() => turns.retry(), String(pending.args.text ?? pending.args.clarified ?? ""));
  };

  // "Begin": the GM narrates the opening scene, as the Opening Turn.
  const begin = () => void store.run(() => ({ opId: BEGIN_OP_ID, result: api.begin() }), null);

  // "End scene": a Closing Turn, streamed as an Intent's Turn.
  const endScene = () => void store.run(() => turns.endScene(), null);

  const renarrate = (turnId: number) =>
    void store.renarrate(renarrateOpId(turnId), () => api.renarrate(turnId));

  const ask = (text: string) => store.ask(() => api.askOoc(text));

  /** One view call after each note command: the Pinned card and the cards change. */
  const marksChanged = async () => {
    try {
      onView(await play.noteChanged());
    } catch (e) {
      store.notify(`Could not reload the story: ${errorText(e)}`);
    }
  };

  const pin = async (target: Target, on: boolean) => {
    try {
      await api.setPin(target, on);
    } catch (e) {
      store.notify(`Could not change the pin: ${errorText(e)}`);
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
      busy={turn.busy}
      startedMs={turn.startedMs}
      stream={turn.stream}
      pendingIntent={turn.pendingIntent}
      answer={turn.answer}
      failure={turn.failure}
      refusal={turn.refusal}
      notice={turn.notice}
      canRetry={turn.canRetry}
      loadCard={api.getEntityCard}
      onAct={act}
      onAsk={ask}
      onCorrect={correct}
      onRetry={retry}
      onDismissFailure={store.dismissFailure}
      onBegin={begin}
      onEndScene={endScene}
      onRenarrate={renarrate}
      onDismissAnswer={store.dismissAnswer}
      onPin={(target, on) => void pin(target, on)}
      onNote={note}
      fixing={fixing}
      onFixing={onFixing}
      tools={tools}
    />
  );
}
