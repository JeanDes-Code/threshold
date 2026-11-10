import { useCallback, useEffect, useState } from "react";
import {
  errorText,
  type CampaignCard,
  type CampaignSummary,
  type DraftLine,
  type PlayView,
} from "./api";
import Home from "./home/Home";
import { navigate, withoutDebug, type AppScreen, type CampaignTab, type Go } from "./home/logic";
import Menu from "./home/Menu";
import LiveJournal from "./journal/LiveJournal";
import LivePlay from "./play/LivePlay";
import { canOpenCorrection } from "./play/logic";
import { api, play, turns, useEvent } from "./tauri";
import Character from "./views/Character";
import Debug from "./views/Debug";
import NewCampaign from "./views/NewCampaign";
import ScreenFrame from "./views/ScreenFrame";

const TABS: readonly (readonly [CampaignTab, string])[] = [
  ["play", "Story"],
  ["journal", "Journal"],
  ["character", "Character"],
];

/**
 * The app opens on home. One screen shows at a time: home, a new Campaign,
 * or the open Campaign (story, Journal, character, debug). Nothing stacks.
 */
export default function App() {
  const [screen, setScreen] = useState<AppScreen>({ kind: "home" });
  const [campaign, setCampaign] = useState<CampaignSummary | null>(null);
  const [view, setView] = useState<PlayView | null>(null);
  const [debug, setDebug] = useState(false);
  const [model, setModel] = useState("warming");
  const [turnBusy, setTurnBusy] = useState(false);
  const [fixing, setFixing] = useState(false);
  const [appError, setAppError] = useState<string | null>(null);
  // Home.
  const [cards, setCards] = useState<CampaignCard[] | null>(null);
  const [drafts, setDrafts] = useState<DraftLine[]>([]);
  const [opening, setOpening] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  // The new-Campaign screen stays mounted while its draft or World Build runs.
  const [newScreen, setNewScreen] = useState<{ key: number; draft: string | null } | null>(null);
  const [job, setJob] = useState(false);

  useEffect(() => {
    api.modelStatus().then(setModel, (e: unknown) => setModel(`unknown (${errorText(e)})`));
  }, []);
  useEvent<string>("model_status", setModel);

  const go = (to: Go) => setScreen((s) => navigate(s, to, { debug, open: campaign !== null }));

  // Each visit to home reads the files again: a World Build may have made one.
  const onHome = screen.kind === "home";
  const readHome = useCallback(() => {
    api.listCampaigns().then(
      (found) => {
        setNow(Date.now());
        setCards(found);
      },
      (e: unknown) => setAppError(`Could not list your Campaigns: ${errorText(e)}`),
    );
    api.listDrafts().then(setDrafts, (e: unknown) => setAppError(`Could not list the drafts: ${errorText(e)}`));
  }, []);
  useEffect(() => {
    if (onHome) readHome();
  }, [onHome, readHome]);

  const opened = (summary: CampaignSummary) => {
    // A Turn that failed in the previous Campaign must not be retried here.
    turns.reset();
    setCampaign(summary);
    setView(null);
    setFixing(false);
    setAppError(null);
    setScreen((s) => navigate(s, { to: "opened" }, { debug, open: true }));
    play.opened().then(setView, (e: unknown) => setAppError(`Could not read the Campaign: ${errorText(e)}`));
  };

  const openCard = async (card: CampaignCard) => {
    if (card.problem !== null || turnBusy) return;
    setOpening(card.db_path);
    setAppError(null);
    try {
      opened(await api.openCampaign(card.db_path));
    } catch (e) {
      setAppError(`Could not open the Campaign: ${errorText(e)}`);
    } finally {
      setOpening(null);
    }
  };

  /** A new Campaign, or a saved draft. A running job keeps its screen as it is. */
  const startNew = (draft: string | null) => {
    if (!job || newScreen === null) setNewScreen((n) => ({ key: (n?.key ?? 0) + 1, draft }));
    go({ to: "new" });
  };

  const toggleDebug = async (on: boolean) => {
    try {
      const confirmed = await api.setDebug(on);
      setDebug(confirmed);
      setAppError(null);
      if (!confirmed) setScreen(withoutDebug);
    } catch (e) {
      // The menu keeps showing the last state the shell confirmed.
      setAppError(`Could not change debug mode: ${errorText(e)}`);
    }
  };

  const story = view?.story ?? [];
  const canCorrect = canOpenCorrection(view?.can_correct ?? false, turnBusy);
  const tab: CampaignTab | null = screen.kind === "campaign" ? screen.tab : null;

  const menu = (inCampaign: boolean) => (
    <Menu
      label={inCampaign ? "Campaign" : "Menu"}
      onHome={inCampaign ? () => go({ to: "home" }) : undefined}
      onCorrect={
        inCampaign
          ? () => {
              setFixing(true);
              go({ to: "tab", tab: "play" });
            }
          : undefined
      }
      canCorrect={canCorrect}
      correctHint={turnBusy ? "Wait until the GM is done" : "Only the latest Turn, when you played it"}
      debug={debug}
      onDebug={(on) => void toggleDebug(on)}
      onDebugView={() => go({ to: "debug" })}
      model={model}
    />
  );

  const tabs: readonly (readonly [CampaignTab, string])[] = debug ? [...TABS, ["debug", "Debug"]] : TABS;
  const tools = (
    <>
      <nav className="view-switch" aria-label="Campaign screens">
        {tabs.map(([id, label]) => (
          <button
            key={id}
            type="button"
            className="view-switch__item"
            aria-current={tab === id ? "page" : undefined}
            onClick={() => go({ to: "tab", tab: id })}
          >
            {label}
          </button>
        ))}
      </nav>
      {menu(true)}
    </>
  );

  return (
    <div className="app">
      {debug && (
        <div className="banner" role="alert">
          DEBUG MODE: this shows GM-only facts, prompts and raw model output.
        </div>
      )}
      {appError && screen.kind !== "home" && <p className="error page">{appError}</p>}
      <div className="app__body">
        {screen.kind === "home" && (
          <Home
            cards={cards}
            drafts={drafts}
            now={now}
            error={appError}
            opening={opening}
            jobRunning={job}
            onOpen={(card) => void openCard(card)}
            onNew={() => startNew(null)}
            onDraft={(id) => startNew(id)}
            menu={menu(false)}
          />
        )}
        {/* Hidden, not unmounted: a running draft or World Build keeps its screen. */}
        {newScreen !== null && (
          <div hidden={screen.kind !== "new"} className="app__screen">
            <ScreenFrame
              label="New Campaign"
              title="From a document"
              onBack={() => go({ to: "home" })}
              backText="Home"
              tools={menu(false)}
            >
              <NewCampaign
                key={newScreen.key}
                initialDraft={newScreen.draft}
                onOpened={opened}
                locked={turnBusy}
                onJob={setJob}
                debug={debug}
              />
            </ScreenFrame>
          </div>
        )}
        {screen.kind === "debug" && debug && (
          <ScreenFrame
            label="Debug"
            title="Prompts and model output"
            onBack={() => go({ to: "home" })}
            backText="Home"
            tools={menu(false)}
          >
            <Debug turns={[]} />
          </ScreenFrame>
        )}
        {campaign && tab === "play" && view && (
          <LivePlay
            key={campaign.db_path}
            view={view}
            onView={setView}
            onBusy={setTurnBusy}
            fixing={fixing}
            onFixing={setFixing}
            tools={tools}
          />
        )}
        {campaign && tab === "journal" && (
          <LiveJournal
            revision={story.length}
            pcName={campaign.pc_name}
            onMarksChanged={() => void play.noteChanged().then(setView, () => undefined)}
            tools={tools}
          />
        )}
        {campaign && tab === "character" && (
          <ScreenFrame label="Character" title={campaign.pc_name} tools={tools}>
            <Character revision={story.length} />
          </ScreenFrame>
        )}
        {campaign && tab === "debug" && debug && (
          <ScreenFrame label="Debug" title="Prompts and model output" tools={tools}>
            <Debug turns={story} />
          </ScreenFrame>
        )}
      </div>
    </div>
  );
}
