// When the Play screen reads the core. The UI calls `get_play_view` once
// after a Campaign opens, once after each Turn ends (committed or failed),
// and once after each note command. A running Turn holds the session lock,
// so no view call runs inside one.

import type { Api, PlayView } from "../api";

export interface PlaySession {
  opened(): Promise<PlayView>;
  turnEnded(): Promise<PlayView>;
  noteChanged(): Promise<PlayView>;
}

export function createPlaySession(api: Pick<Api, "getPlayView">): PlaySession {
  const load = () => api.getPlayView();
  return { opened: load, turnEnded: load, noteChanged: load };
}
