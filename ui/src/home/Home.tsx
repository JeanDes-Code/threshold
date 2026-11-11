import { useState, type ReactNode } from "react";
import type { CampaignCard, DraftLine, DraftState } from "../api";
import { cardLine, lastPlayedText, problemTitle, startedText, unfinishedDrafts } from "./logic";

interface Props {
  /** null while the cards load. */
  cards: CampaignCard[] | null;
  drafts: DraftLine[];
  /** For "played 3 hours ago". */
  now: number;
  error: string | null;
  /** The card being opened, if any. */
  opening: string | null;
  /** A draft or a World Build runs: the new-Campaign screen keeps it. */
  jobRunning: boolean;
  onOpen: (card: CampaignCard) => void;
  /** "Remove from list", after the Player confirmed: the file moves to the archive folder. */
  onRemove: (card: CampaignCard) => void;
  onNew: () => void;
  onDraft: (draftId: string) => void;
  /** The menu: debug mode and the model status. */
  menu: ReactNode;
}

const DRAFT_STATE: Record<DraftState, string> = {
  draft: "To check",
  approved: "Ready to prepare",
  built: "Built",
};

/** The first screen: the Player's Campaigns as cards, unfinished drafts, one way to start. */
export default function Home(props: Props) {
  const { cards, now, error, opening, jobRunning, onOpen, onRemove, onNew, onDraft, menu } = props;
  const drafts = unfinishedDrafts(props.drafts);
  return (
    <div className="home">
      <header className="home-top">
        <div className="home-top__brand">
          <span className="home-top__sigil" aria-hidden="true" />
          <span className="home-top__name">Threshold</span>
        </div>
        {menu}
      </header>
      <main className="home-page">
        <div className="home-page__measure">
          <section className="home-hero">
            <div>
              <p className="play-label">Your table</p>
              <h1 className="home-hero__title">Where shall we go on?</h1>
            </div>
            <button type="button" className="play-btn play-btn--primary home-hero__new" onClick={onNew}>
              {jobRunning ? "Back to the new Campaign" : "New Campaign"}
            </button>
          </section>

          {error !== null && (
            <p className="play-alert play-alert--error" role="alert">
              {error}
            </p>
          )}

          <section className="home-section" aria-label="Your Campaigns">
            <h2 className="play-label">Campaigns</h2>
            {cards === null ? (
              <p className="play-empty">Reading your Campaigns…</p>
            ) : cards.length === 0 ? (
              <div className="home-empty">
                <p className="home-empty__title">No Campaign yet.</p>
                <p className="play-empty">
                  Start one from a document: a chronicle, a premise, notes from another table.
                </p>
              </div>
            ) : (
              <ul className="home-cards">
                {cards.map((card) => (
                  <li key={card.db_path}>
                    <Card
                      card={card}
                      now={now}
                      opening={opening === card.db_path}
                      disabled={opening !== null}
                      onOpen={() => onOpen(card)}
                      onRemove={() => onRemove(card)}
                    />
                  </li>
                ))}
              </ul>
            )}
          </section>

          {drafts.length > 0 && (
            <section className="home-section" aria-label="Unfinished drafts">
              <h2 className="play-label">Unfinished drafts</h2>
              <ul className="home-drafts">
                {drafts.map((draft) => (
                  <li key={draft.draft_id} className="home-draft">
                    <span className="home-draft__title">{draft.title || "A draft with no premise yet"}</span>
                    <span className="home-draft__state">{DRAFT_STATE[draft.state] ?? draft.state}</span>
                    <button
                      type="button"
                      className="play-btn play-btn--outline home-draft__go"
                      disabled={jobRunning}
                      onClick={() => onDraft(draft.draft_id)}
                    >
                      Continue
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </main>
    </div>
  );
}

interface CardProps {
  card: CampaignCard;
  now: number;
  opening: boolean;
  disabled: boolean;
  onOpen: () => void;
  onRemove: () => void;
}

function Card({ card, now, opening, disabled, onOpen, onRemove }: CardProps) {
  if (card.problem !== null) {
    return <ProblemCard card={card} disabled={disabled} onRemove={onRemove} />;
  }
  const started = startedText(card.db_path);
  return (
    <button
      type="button"
      className="home-card"
      disabled={disabled}
      aria-label={`Open ${card.pc_name}${started ? `, ${started}` : ""}: ${cardLine(card)}, ${lastPlayedText(card.last_played_ms, now)}`}
      onClick={onOpen}
    >
      <span className="home-card__head">
        <span className="play-initial home-card__initial" aria-hidden="true">
          {card.pc_name.charAt(0).toUpperCase() || "?"}
        </span>
        <span className="home-card__name">{card.pc_name || "Unnamed"}</span>
      </span>
      {started !== null && <span className="home-card__started">{started}</span>}
      <span className="home-card__premise">{card.premise}</span>
      <span className="home-card__foot">
        <span className={card.needs_begin ? "home-card__count home-card__count--new" : "home-card__count"}>
          {cardLine(card)}
        </span>
        <span className="home-card__when">{opening ? "Opening…" : lastPlayedText(card.last_played_ms, now)}</span>
      </span>
    </button>
  );
}

/** A file that cannot open. "Remove from list" asks once, then moves the file to the archive folder. */
function ProblemCard({ card, disabled, onRemove }: { card: CampaignCard; disabled: boolean; onRemove: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const name = problemTitle(card.db_path);
  return (
    <article className="home-card home-card--problem" aria-label={`${name}: cannot open`}>
      <p className="play-label">Cannot open</p>
      <h3 className="home-card__name">{name}</h3>
      <p className="home-card__problem">{cardLine(card)}</p>
      {confirming ? (
        <div className="home-card__confirm" role="group" aria-label={`Remove ${name} from the list?`}>
          <p className="play-empty">The file moves to the archive folder of the app. Nothing is deleted.</p>
          <button
            type="button"
            className="play-btn play-btn--outline"
            disabled={disabled}
            onClick={() => {
              setConfirming(false);
              onRemove();
            }}
          >
            Move it to the archive
          </button>
          <button type="button" className="play-btn play-btn--text" onClick={() => setConfirming(false)}>
            Keep it
          </button>
        </div>
      ) : (
        <button type="button" className="play-btn play-btn--text" disabled={disabled} onClick={() => setConfirming(true)}>
          Remove from list
        </button>
      )}
    </article>
  );
}
