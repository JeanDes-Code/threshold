import { useCallback, useEffect, useState } from "react";
import { errorText, type JournalView } from "../api";
import { api } from "../tauri";
import Journal, { type JournalActions } from "./Journal";

interface Props {
  /** Changes after each Turn: the Journal is read again. */
  revision: number;
  pcName: string;
  /** Told after each note or pin command, so the Play screen reads its view again. */
  onMarksChanged: () => void;
}

/** The Journal of the open Campaign, from `get_journal`. */
export default function LiveJournal({ revision, pcName, onMarksChanged }: Props) {
  const [view, setView] = useState<JournalView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api.getJournal().then(setView, (e: unknown) => setError(errorText(e)));
  }, []);
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
  if (!view) return <p className="muted page">Loading…</p>;
  return <Journal view={view} pcName={pcName} actions={actions} />;
}
