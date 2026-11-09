import { useEffect, useState } from "react";
import { errorText, type CampaignSummary, type Defaults, type TurnLine } from "./api";
import { api, turns, useEvent } from "./tauri";
import Character from "./views/Character";
import Debug from "./views/Debug";
import Journal from "./views/Journal";
import Play from "./views/Play";

type Tab = "play" | "character" | "journal" | "debug";

export default function App() {
  const [campaign, setCampaign] = useState<CampaignSummary | null>(null);
  const [lines, setLines] = useState<TurnLine[]>([]);
  const [tab, setTab] = useState<Tab>("play");
  const [debug, setDebug] = useState(false);
  const [model, setModel] = useState("warming");
  const [showSetup, setShowSetup] = useState(true);
  const [turnBusy, setTurnBusy] = useState(false);
  const [appError, setAppError] = useState<string | null>(null);

  useEffect(() => {
    api.modelStatus().then(setModel, (e: unknown) => setModel(`unknown (${errorText(e)})`));
  }, []);
  useEvent<string>("model_status", setModel);

  const opened = (summary: CampaignSummary) => {
    // A Turn that failed in the previous campaign must not be retried here.
    turns.reset();
    setCampaign(summary);
    setLines(summary.turns);
    setShowSetup(false);
    setTab("play");
  };

  const toggleDebug = async (on: boolean) => {
    try {
      const now = await api.setDebug(on);
      setDebug(now);
      setAppError(null);
      if (!now && tab === "debug") setTab("play");
    } catch (e) {
      // The checkbox keeps showing the last state the shell confirmed.
      setAppError(`Could not change debug mode: ${errorText(e)}`);
    }
  };

  return (
    <>
      {debug && (
        <div className="banner" role="alert">
          DEBUG MODE: this shows GM-only facts, prompts and raw model output.
        </div>
      )}
      <nav className="menu">
        <strong>Threshold</strong>
        <button onClick={() => setShowSetup((s) => !s)}>Campaign…</button>
        <label className="toggle">
          <input
            type="checkbox"
            checked={debug}
            onChange={(e) => void toggleDebug(e.target.checked)}
          />
          Debug mode
        </label>
        <span className="spacer" />
        <span className="status">Model: {model}</span>
      </nav>
      {appError && <p className="error page">{appError}</p>}
      {showSetup && <Setup onOpened={opened} locked={turnBusy} />}
      {campaign && (
        <>
          <div className="tabs">
            {(["play", "character", "journal"] as const).map((t) => (
              <button key={t} aria-pressed={tab === t} onClick={() => setTab(t)}>
                {t[0]?.toUpperCase() + t.slice(1)}
              </button>
            ))}
            {debug && (
              <button aria-pressed={tab === "debug"} onClick={() => setTab("debug")}>
                Debug
              </button>
            )}
          </div>
          <main>
            {tab === "play" && (
              <Play
                key={campaign.db_path}
                campaign={campaign}
                onBusy={setTurnBusy}
                lines={lines}
                onLine={(line) => setLines((all) => [...all, line])}
                onRefresh={() => {
                  api.summary().then(
                    (summary) => setLines(summary.turns),
                    (e: unknown) => setAppError(`Could not reload the Turns: ${errorText(e)}`),
                  );
                }}
                onNarration={(turnId, narration) =>
                  setLines((all) =>
                    all.map((l) => (l.turn_id === turnId ? { ...l, narration } : l)),
                  )
                }
              />
            )}
            {tab === "character" && <Character revision={lines.length} />}
            {tab === "journal" && <Journal revision={lines.length} />}
            {tab === "debug" && debug && <Debug lines={lines} />}
          </main>
        </>
      )}
    </>
  );
}

interface SetupProps {
  onOpened: (summary: CampaignSummary) => void;
  /** True while a Turn runs: no campaign may open then. */
  locked: boolean;
}

function Setup({ onOpened, locked }: SetupProps) {
  const [defaults, setDefaults] = useState<Defaults | null>(null);
  const [seedPath, setSeedPath] = useState("");
  const [worldPath, setWorldPath] = useState("");
  const [dbPath, setDbPath] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.defaults().then(
      (d) => {
        setDefaults(d);
        setSeedPath(d.seed_path);
        setWorldPath(d.world_path);
      },
      (e: unknown) => setError(`Could not read the default paths: ${errorText(e)}`),
    );
    api.listCampaigns().then(setFiles, (e: unknown) =>
      setError(`Could not list campaign files: ${errorText(e)}`),
    );
  }, []);

  const run = async (call: () => Promise<CampaignSummary>) => {
    setBusy(true);
    setError(null);
    try {
      onOpened(await call());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main>
      <h2>New campaign</h2>
      <label htmlFor="seed">Adventure Seed file</label>
      <input id="seed" value={seedPath} onChange={(e) => setSeedPath(e.target.value)} />
      <label htmlFor="world">World file</label>
      <input id="world" value={worldPath} onChange={(e) => setWorldPath(e.target.value)} />
      <div className="row">
        <button
          disabled={busy || locked}
          onClick={() => void run(() => api.newCampaign(seedPath, worldPath))}
        >
          New campaign
        </button>
      </div>

      <h2>Open campaign</h2>
      <label htmlFor="db">Campaign file</label>
      <input id="db" value={dbPath} onChange={(e) => setDbPath(e.target.value)} />
      <div className="row">
        <button
          disabled={busy || locked || !dbPath}
          onClick={() => void run(() => api.openCampaign(dbPath))}
        >
          Open
        </button>
      </div>
      {files.length > 0 && (
        <ul>
          {files.map((f) => (
            <li key={f}>
              <button className="muted" onClick={() => setDbPath(f)}>
                {f}
              </button>
            </li>
          ))}
        </ul>
      )}
      {defaults && <p className="muted">Data and config (threshold.toml): {defaults.data_dir}</p>}
      {locked && <p className="muted">A Turn is running; wait for it to finish.</p>}
      {error && <p className="error">{error}</p>}
    </main>
  );
}
