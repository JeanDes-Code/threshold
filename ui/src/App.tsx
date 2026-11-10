import { useEffect, useState } from "react";
import { errorText, type CampaignSummary, type PlayView } from "./api";
import LivePlay from "./play/LivePlay";
import { api, play, turns, useEvent } from "./tauri";
import Character from "./views/Character";
import Debug from "./views/Debug";
import Journal from "./views/Journal";
import NewCampaign from "./views/NewCampaign";

type Tab = "play" | "character" | "journal" | "debug";

export default function App() {
  const [campaign, setCampaign] = useState<CampaignSummary | null>(null);
  const [view, setView] = useState<PlayView | null>(null);
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
    setView(null);
    setShowSetup(false);
    setTab("play");
    play.opened().then(setView, (e: unknown) =>
      setAppError(`Could not read the Campaign: ${errorText(e)}`),
    );
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

  const story = view?.story ?? [];

  return (
    <div className="app">
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
      {(campaign || debug) && (
        <div className="tabs">
          {campaign &&
            (["play", "character", "journal"] as const).map((t) => (
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
      )}
      <div className="app__body">
        {/* Hidden, not unmounted: a running draft or build keeps its screen. */}
        <div hidden={!showSetup}>
          <Setup onOpened={opened} locked={turnBusy} shown={showSetup} />
        </div>
        {/* The debug view works without a Campaign: a failed World Build has none. */}
        {tab === "debug" && debug && (
          <main>
            <Debug turns={story} />
          </main>
        )}
        {campaign && tab === "play" && view && (
          <LivePlay key={campaign.db_path} view={view} onView={setView} onBusy={setTurnBusy} />
        )}
        {campaign && (tab === "character" || tab === "journal") && (
          <main>
            {tab === "character" && <Character revision={story.length} />}
            {tab === "journal" && <Journal revision={story.length} />}
          </main>
        )}
      </div>
    </div>
  );
}

interface SetupProps {
  onOpened: (summary: CampaignSummary) => void;
  /** True while a Turn runs: no campaign may open then. */
  locked: boolean;
  /** The screen stays mounted while hidden; each showing lists the files again. */
  shown: boolean;
}

function Setup({ onOpened, locked, shown }: SetupProps) {
  const [dataDir, setDataDir] = useState<string | null>(null);
  const [dbPath, setDbPath] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.defaults().then(
      (d) => setDataDir(d.data_dir),
      (e: unknown) => setError(`Could not read the data folder: ${errorText(e)}`),
    );
  }, []);

  // A Campaign that a World Build made since the last look is listed too.
  useEffect(() => {
    if (!shown) return;
    api.listCampaigns().then(setFiles, (e: unknown) =>
      setError(`Could not list campaign files: ${errorText(e)}`),
    );
  }, [shown]);

  const open = async () => {
    setBusy(true);
    setError(null);
    try {
      onOpened(await api.openCampaign(dbPath));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main>
      <NewCampaign onOpened={onOpened} locked={locked} />

      <h2>Open campaign</h2>
      <label htmlFor="db">Campaign file</label>
      <input id="db" value={dbPath} onChange={(e) => setDbPath(e.target.value)} />
      <div className="row">
        <button disabled={busy || locked || !dbPath} onClick={() => void open()}>
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
      {dataDir && <p className="muted">Data and config (threshold.toml): {dataDir}</p>}
      {locked && <p className="muted">A Turn is running; wait for it to finish.</p>}
      {error && <p className="error">{error}</p>}
    </main>
  );
}
