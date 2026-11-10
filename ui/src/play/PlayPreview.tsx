// Dev only: the Play screen on the invented fixture, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=play while `npm run dev` runs.
// `&busy=deciding` or `&busy=narrating` opens it in that state.

import { useEffect, useRef, useState } from "react";
import type { PlayView } from "../api";
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
        });
    };
    later(1200, feed);
  };

  const cycleBusy = () => {
    if (replaying) return;
    setStep((s) => (s === "idle" ? "deciding" : s === "deciding" ? "narrating" : "idle"));
  };

  const busy = replaying || step !== "idle";
  const shownStream = replaying ? stream : step === "narrating" ? halfStream : null;
  // While a Turn runs, the story stops before the last Turn, as it would live.
  const view = busy ? beforeLast : playView;
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
        stream={shownStream}
        pendingIntent={pendingIntent}
        answer={answer}
        onAct={async () => {
          replay();
          return true;
        }}
        onAsk={async () => {
          setAnswer("The GM answers here, out of character. This box is not part of the story.");
          return true;
        }}
        onBegin={replay}
        onRenarrate={replay}
        onDismissAnswer={() => setAnswer(null)}
      />
    </div>
  );
}
