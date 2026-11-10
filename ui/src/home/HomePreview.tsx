// Dev only: the home screen on invented cards, with no Tauri call.
// Open http://127.0.0.1:5173/?preview=home while `npm run dev` runs.
// `&menu=open` opens the menu; `&empty=1` shows a first start.

import { useState } from "react";
import { homeCards, homeDrafts, homeNow } from "../fixtures/home";
import Home from "./Home";
import Menu from "./Menu";

export default function HomePreview() {
  const params = new URLSearchParams(window.location.search);
  const empty = params.get("empty") === "1";
  const [debug, setDebug] = useState(false);
  const [opening, setOpening] = useState<string | null>(null);
  return (
    <div className="preview">
      <div className="preview__strip">
        <span className="preview__label">Preview · fixture data</span>
      </div>
      <Home
        cards={empty ? [] : homeCards}
        drafts={empty ? [] : homeDrafts}
        now={homeNow}
        error={null}
        opening={opening}
        jobRunning={false}
        onOpen={(card) => setOpening(card.db_path)}
        onNew={() => undefined}
        onDraft={() => undefined}
        menu={
          <Menu
            label="Menu"
            debug={debug}
            onDebug={setDebug}
            model="warming"
            initiallyOpen={params.get("menu") === "open"}
          />
        }
      />
    </div>
  );
}
