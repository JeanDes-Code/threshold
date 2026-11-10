import type { ReactNode } from "react";

interface Props {
  /** The small label above the title. */
  label: string;
  title: string;
  /** "Back" and where it goes; none on a screen with nowhere to go back to. */
  onBack?: () => void;
  backText?: string;
  /** The screen switch and the menu, on the right. */
  tools?: ReactNode;
  children: ReactNode;
}

/** A screen with the same top bar and measure as the Play screen and the Journal. */
export default function ScreenFrame({ label, title, onBack, backText = "Back", tools, children }: Props) {
  return (
    <div className="screen">
      <header className="screen-top">
        <div className="screen-top__title">
          {onBack && (
            <button type="button" className="play-btn play-btn--text screen-top__back" onClick={onBack}>
              ‹ {backText}
            </button>
          )}
          <div>
            <p className="play-label">{label}</p>
            <h1 className="screen-top__name">{title}</h1>
          </div>
        </div>
        <div className="screen-top__tools">{tools}</div>
      </header>
      <main className="screen-page">
        <div className="screen-page__measure">{children}</div>
      </main>
    </div>
  );
}
