// Dev only: the Play screen on the invented fixture, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=play while `npm run dev` runs.
// `&busy=deciding` or `&busy=narrating` opens it in that state.

import { useEffect, useRef, useState } from "react";
import type { PinLine, PlayView, Target } from "../api";
import Menu from "../home/Menu";
import { sameTarget } from "../marks/logic";
import { fixtureCard } from "../fixtures/entityCards";
import { playView, streamText } from "../fixtures/playView";
import PlayScreen from "./PlayScreen";

type Step = "idle" | "deciding" | "narrating";

const lastTurn = playView.story.at(-1);
const beforeLast: PlayView = { ...playView, story: playView.story.slice(0, -1) };
const halfStream = streamText.slice(0, streamText.indexOf("Orsel:") + 30);

function initialStep(): Step {
  const busy = new URLSearchParams(window.location.search).get("busy");
  return busy === "deciding" || busy === "narrating" ? busy : "idle";
}

export default function PlayPreview() {
  const [step, setStep] = useState<Step>(initialStep);
  const [replaying, setReplaying] = useState(false);
  const [stream, setStream] = useState<string | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  const [pinned, setPinned] = useState<PinLine[]>(playView.pinned);
  const [fixing, setFixing] = useState(false);
  const [debug, setDebug] = useState(false);
  // The preview counts from when it turned busy, as the session store does.
  const [startedMs, setStartedMs] = useState<number | null>(() => (initialStep() === "idle" ? null : Date.now()));
  const timers = useRef<number[]>([]);

  useEffect(() => () => timers.current.forEach((t) => window.clearTimeout(t)), []);

  const later = (ms: number, fn: () => void) => {
    timers.current.push(window.setTimeout(fn, ms));
  };

  // Feeds the raw stream in chunks, marks visible, then swaps to the committed blocks.
  const replay = () => {
    if (replaying) return;
    setStep("idle");
    setReplaying(true);
    setStartedMs(Date.now());
    setStream("");
    let at = 0;
    const feed = () => {
      at = Math.min(streamText.length, at + 3 + Math.floor(Math.random() * 7));
      setStream(streamText.slice(0, at));
      if (at < streamText.length) later(32, feed);
      else
        later(700, () => {
          setStream(null);
          setReplaying(false);
          setStartedMs(null);
        });
    };
    later(1200, feed);
  };

  const cycleBusy = () => {
    if (replaying) return;
    const next: Step = step === "idle" ? "deciding" : step === "deciding" ? "narrating" : "idle";
    setStep(next);
    if (step === "idle") setStartedMs(Date.now());
    if (next === "idle") setStartedMs(null);
  };

  const busy = replaying || step !== "idle";
  const shownStream = replaying ? stream : step === "narrating" ? halfStream : step === "deciding" ? "" : null;
  // While a Turn runs, the story stops before the last Turn, as it would live.
  const view = { ...(busy ? beforeLast : playView), pinned };

  // The preview keeps Pins in memory: the core would give a title and text.
  const pin = (target: Target, on: boolean) =>
    setPinned((list) =>
      on
        ? [...list, { target, title: "Pinned in the preview", text: "" }]
        : list.filter((p) => !sameTarget(p.target, target)),
    );
  const pendingIntent = busy ? (lastTurn?.intent ?? null) : null;

  return (
    <div className="preview">
      <div className="preview__strip" role="toolbar" aria-label="Preview controls">
        <span className="preview__label">Preview · fixture data</span>
        <button type="button" className="play-btn play-btn--text" disabled={replaying} onClick={replay}>
          Replay the last Turn's stream
        </button>
        <button type="button" className="play-btn play-btn--text" disabled={replaying} onClick={cycleBusy}>
          Busy: {step}
        </button>
      </div>
      <PlayScreen
        view={view}
        busy={busy}
        startedMs={startedMs}
        stream={shownStream}
        pendingIntent={pendingIntent}
        answer={answer}
        failure={null}
        refusal={null}
        notice={null}
        canRetry={false}
        loadCard={fixtureCard}
        onAct={async () => {
          replay();
          return true;
        }}
        onCorrect={async () => {
          replay();
          return true;
        }}
        onRetry={async () => {
          replay();
          return true;
        }}
        onDismissFailure={() => undefined}
        onAsk={async () => {
          setAnswer("The GM answers here, out of character. This box is not part of the story.");
          return true;
        }}
        onBegin={replay}
        onEndScene={replay}
        onFade={replay}
        onRenarrate={replay}
        onDismissAnswer={() => setAnswer(null)}
        onPin={pin}
        onNote={async () => null}
        fixing={fixing}
        onFixing={setFixing}
        tools={
          <>
            <nav className="view-switch" aria-label="Campaign screens">
              <button type="button" className="view-switch__item" aria-current="page">
                Story
              </button>
              <button type="button" className="view-switch__item">
                Journal
              </button>
              <button type="button" className="view-switch__item">
                Character
              </button>
            </nav>
            <Menu
              label="Campaign"
              onHome={() => undefined}
              onCorrect={() => setFixing(true)}
              canCorrect={!busy && view.can_correct}
              correctHint="Wait until the GM is done"
              debug={debug}
              onDebug={setDebug}
              model="ready"
              initiallyOpen={new URLSearchParams(window.location.search).get("menu") === "open"}
            />
          </>
        }
      />
    </div>
  );
}
