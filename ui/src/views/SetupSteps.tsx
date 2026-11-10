const STEPS = [
  ["source", "Your document"],
  ["review", "Check the draft"],
  ["build", "Prepare the world"],
] as const;

export type SetupStep = (typeof STEPS)[number][0];

/** Where the Player is in the setup of a new Campaign: three steps, one current. */
export default function SetupSteps({ current }: { current: SetupStep }) {
  const at = STEPS.findIndex(([id]) => id === current);
  return (
    <ol className="setup-steps" aria-label="New Campaign steps">
      {STEPS.map(([id, label], n) => (
        <li
          key={id}
          className={`setup-steps__item${n < at ? " setup-steps__item--done" : ""}`}
          aria-current={n === at ? "step" : undefined}
        >
          <span className="setup-steps__num" aria-hidden="true">
            {n < at ? "✓" : n + 1}
          </span>
          {label}
        </li>
      ))}
    </ol>
  );
}
