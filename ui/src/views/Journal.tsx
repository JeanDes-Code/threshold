import { useEffect, useState } from "react";
import { errorText, type JournalEntry, type JournalView } from "../api";
import { api } from "../tauri";

/**
 * What the PC knows: observations (witnessed) apart from Claims (told). The
 * shell builds it from PC Knowledge only, so no GM-only fact reaches it.
 */
export default function Journal({ revision }: { revision: number }) {
  const [view, setView] = useState<JournalView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getJournal().then(setView, (e: unknown) => setError(errorText(e)));
  }, [revision]);

  if (error) return <p className="error">{error}</p>;
  if (!view) return <p className="muted">Loading…</p>;
  return (
    <section>
      <h2>Observations</h2>
      <p className="muted">What the PC saw for themself.</p>
      <Entries entries={view.observations} />
      <h2>Claims</h2>
      <p className="muted">What the PC was told. A Claim can be false.</p>
      <Entries entries={view.claims} />
    </section>
  );
}

function Entries({ entries }: { entries: JournalEntry[] }) {
  if (entries.length === 0) return <p className="muted">Nothing yet.</p>;
  return (
    <ul>
      {entries.map((e, i) => (
        <li key={i}>
          {e.text}
          {e.source && <span className="muted"> (said by {e.source})</span>}
        </li>
      ))}
    </ul>
  );
}
