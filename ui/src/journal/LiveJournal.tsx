import { useEffect, useState } from "react";
import { errorText, type JournalView } from "../api";
import { api } from "../tauri";
import Journal from "./Journal";

interface Props {
  /** Changes after each Turn: the Journal is read again. */
  revision: number;
  pcName: string;
}

/** The Journal of the open Campaign, from `get_journal`. */
export default function LiveJournal({ revision, pcName }: Props) {
  const [view, setView] = useState<JournalView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getJournal().then(setView, (e: unknown) => setError(errorText(e)));
  }, [revision]);

  if (error) return <p className="error page">{error}</p>;
  if (!view) return <p className="muted page">Loading…</p>;
  return <Journal view={view} pcName={pcName} />;
}
