import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import type { PackLine } from "../api";
import { modelState, modelText, packLineText } from "./logic";

interface Props {
  /** "Campaign" in a Campaign, "Menu" elsewhere. */
  label: string;
  /** Home: shown in a Campaign. */
  onHome?: () => void;
  /** Correct last Turn: shown in a Campaign. */
  onCorrect?: () => void;
  /** The latest Turn can be corrected now (a standing Play Turn, no Turn running). */
  canCorrect?: boolean;
  /** Why it cannot, said next to the item. */
  correctHint?: string;
  debug: boolean;
  onDebug: (on: boolean) => void;
  /** Opens the debug view: shown while debug mode is on. */
  onDebugView?: () => void;
  /** The model status the shell reports. */
  model: string;
  /** The Content Packs of the data folder; null until the shell answers. */
  packs?: PackLine[] | null;
  /** Opens the menu on the first render (dev preview only). */
  initiallyOpen?: boolean;
}

/**
 * The Campaign menu of the top bar: Home, Correct last Turn, debug mode and
 * the model status. Arrow keys move between items; Escape closes it.
 */
export default function Menu(props: Props) {
  const { label, debug, model } = props;
  const [open, setOpen] = useState(props.initiallyOpen ?? false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  // A click outside closes it.
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);

  const close = () => {
    setOpen(false);
    button.current?.focus();
  };

  const run = (action: () => void) => () => {
    setOpen(false);
    action();
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = Array.from(root.current?.querySelectorAll<HTMLElement>("[role^=menuitem]:not(:disabled)") ?? []);
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next = e.key === "ArrowDown" ? (at + 1) % items.length : (at - 1 + items.length) % items.length;
    items[next]?.focus();
  };

  const state = modelState(model);
  return (
    <div className="app-menu" ref={root} onKeyDown={onKey}>
      <button
        ref={button}
        type="button"
        className="play-btn play-btn--outline app-menu__button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={menuId}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={`app-menu__dot app-menu__dot--${state}`} aria-hidden="true" />
        {label}
        <span className="app-menu__caret" aria-hidden="true" />
      </button>
      {open && (
        <div className="app-menu__list" role="menu" id={menuId} aria-label={label}>
          {props.onHome && (
            <button type="button" role="menuitem" className="app-menu__item" autoFocus onClick={run(props.onHome)}>
              <span>Home</span>
              <span className="app-menu__sub">All your Campaigns</span>
            </button>
          )}
          {props.onCorrect && (
            <button
              type="button"
              role="menuitem"
              className="app-menu__item"
              disabled={!props.canCorrect}
              onClick={run(props.onCorrect)}
            >
              <span>Correct last Turn</span>
              <span className="app-menu__sub">
                {props.canCorrect ? "Say what you meant; the GM plays it again" : props.correctHint}
              </span>
            </button>
          )}
          <div className="app-menu__rule" role="separator" />
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={debug}
            className="app-menu__item app-menu__item--check"
            autoFocus={!props.onHome}
            onClick={() => props.onDebug(!debug)}
          >
            <span className={debug ? "app-menu__box app-menu__box--on" : "app-menu__box"} aria-hidden="true" />
            <span>
              Debug mode
              <span className="app-menu__sub">Shows GM-only facts and prompts</span>
            </span>
          </button>
          {debug && props.onDebugView && (
            <button type="button" role="menuitem" className="app-menu__item" onClick={run(props.onDebugView)}>
              <span>Open the debug view</span>
            </button>
          )}
          {props.packs && (
            <>
              <div className="app-menu__rule" role="separator" />
              <section className="app-menu__packs" aria-label="Content Packs">
                <p className="app-menu__status">Content Packs</p>
                {props.packs.length === 0 ? (
                  <p className="app-menu__status">No Content Pack in the packs folder</p>
                ) : (
                  <ul className="app-menu__pack-list">
                    {props.packs.map((line) => {
                      const shown = packLineText(line);
                      return (
                        <li key={line.folder} className="app-menu__pack">
                          {shown.title}
                          <span className={shown.refused ? "app-menu__sub app-menu__sub--problem" : "app-menu__sub"}>
                            {shown.detail}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            </>
          )}
          <div className="app-menu__rule" role="separator" />
          <p className="app-menu__status" role="status">
            <span className={`app-menu__dot app-menu__dot--${state}`} aria-hidden="true" />
            {modelText(model)}
          </p>
        </div>
      )}
    </div>
  );
}
