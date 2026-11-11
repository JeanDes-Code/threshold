import { useCallback, useEffect, useState, type ReactNode } from "react";
import { errorText, type JournalView } from "../api";
import { api } from "../tauri";
import Journal, { type JournalActions } from "./Journal";

interface Props {
  /** Changes after each Turn: the Journal is read again. */
  revision: number;
  /** The open Campaign's file: the last Journal read is kept per Campaign. */
  campaign: string;
  /** A Turn is running: the Journal keeps the last one read until it ends. */
  running: boolean;
  pcName: string;
  /** Told after each note or pin command, so the Play screen reads its view again. */
  onMarksChanged: () => void;
  /** The screen switch and the Campaign menu. */
  tools: ReactNode;
}

/** The last Journal read, per Campaign file: shown at once when the tab
 * opens again, even while a Turn runs and `get_journal` waits for it (bug
 * pass 5: the tab showed a bare "Loading…" for the whole Turn). */
let lastRead: { campaign: string; view: JournalView } | null = null;

/** The line above a Journal that a running Turn will change. */
export const RUNNING_LINE = "The GM is playing a Turn. The Journal shows the story up to the last Turn.";

/** The Journal of the open Campaign, from `get_journal`. */
export default function LiveJournal({
  revision,
  campaign,
  running,
  pcName,
  onMarksChanged,
  tools,
}: Props) {
  const [view, setView] = useState<JournalView | null>(() =>
    lastRead?.campaign === campaign ? lastRead.view : null,
  );
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api.getJournal().then(
      (read) => {
        lastRead = { campaign, view: read };
        setView(read);
      },
      (e: unknown) => setError(errorText(e)),
    );
  }, [campaign]);
  useEffect(load, [load, revision]);

  /** Runs one note command, then reads the Journal again. */
  const command = async (call: () => Promise<unknown>): Promise<string | null> => {
    try {
      await call();
    } catch (e) {
      return errorText(e);
    }
    load();
    onMarksChanged();
    return null;
  };

  const actions: JournalActions = {
    onPin: (target, on) =>
      void command(() => api.setPin(target, on)).then((refused) => {
        if (refused !== null) setError(refused);
      }),
    onAddNote: (body, about) => command(() => api.addNote(body, about)),
    onEditNote: (noteId, body) => command(() => api.editNote(noteId, body)),
    onDeleteNote: (noteId) => command(() => api.deleteNote(noteId)),
  };

  if (error) return <p className="error page">{error}</p>;
  if (!view) {
    return <p className="muted page">{running ? RUNNING_LINE : "Loading…"}</p>;
  }
  return (
    <>
      {running && <p className="muted page journal-running">{RUNNING_LINE}</p>}
      <Journal view={view} pcName={pcName} actions={actions} tools={tools} />
    </>
  );
}
