import type { ReactNode } from "react";
import { clockText, type PlaceLine } from "../api";

interface Props {
  place: PlaceLine | null;
  clockMin: number;
  panelOpen: boolean;
  onTogglePanel: () => void;
  /** The screen switch and the Campaign menu. */
  tools?: ReactNode;
}

/** The place on the left, the World Clock on the right, a thin rule under both. */
export default function TopBar({ place, clockMin, panelOpen, onTogglePanel, tools }: Props) {
  return (
    <header className="play-top">
      <div className="play-top__place">
        <span className="play-label">Place</span>
        <span className="play-top__name">{place?.name ?? "Somewhere unknown"}</span>
      </div>
      <div className="play-top__right">
        <time className="play-top__clock">{clockText(clockMin)}</time>
        <button
          type="button"
          className="play-btn play-btn--quiet play-top__panel"
          aria-expanded={panelOpen}
          aria-controls="play-panel"
          onClick={onTogglePanel}
        >
          Panel
        </button>
        {tools}
      </div>
    </header>
  );
}
