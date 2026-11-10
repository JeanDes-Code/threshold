interface Props {
  pinned: boolean;
  /** What the Pin keeps in view, for the button's name: "Pin {label}". */
  label: string;
  disabled?: boolean;
  onToggle: (on: boolean) => void;
}

/** A small pin toggle. A pinned item shows in the Play screen's Pinned card. */
export default function PinButton({ pinned, label, disabled = false, onToggle }: Props) {
  const name = pinned ? `Unpin ${label}` : `Pin ${label}`;
  return (
    <button
      type="button"
      className={pinned ? "mark-pin mark-pin--on" : "mark-pin"}
      aria-pressed={pinned}
      aria-label={name}
      title={pinned ? "Pinned: shown in play. Click to unpin." : "Pin it: keep it in view while you play."}
      disabled={disabled}
      onClick={() => onToggle(!pinned)}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path
          d="M5.5 1.5h5l-.8 4.3 2.8 2.7v1h-9v-1l2.8-2.7z"
          fill={pinned ? "currentColor" : "none"}
          stroke="currentColor"
          strokeWidth="1.3"
          strokeLinejoin="round"
        />
        <path d="M8 9.5v5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      </svg>
    </button>
  );
}
