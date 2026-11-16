import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import SourceStep from "./SourceStep";

const noop = () => undefined;

function render(failed: boolean, error: string | null): string {
  return renderToStaticMarkup(
    <SourceStep
      text="The Ashford Road"
      onText={noop}
      onFile={noop}
      onDraft={noop}
      busy={false}
      running={null}
      error={error}
      failed={failed}
    />,
  );
}

describe("SourceStep", () => {
  // QA run 5 (403): a failed draft showed its error under a "Draft" button,
  // with no word that a new try may work.
  it("offers Retry after a failed draft, with the error", () => {
    const error =
      "The draft stopped while reading the characters: the model's answer did not pass the app's checks. Press Retry to draft again.";
    const html = render(true, error);
    expect(html).toContain(">Retry</button>");
    expect(html).not.toContain(">Draft</button>");
    expect(html.replaceAll("&#x27;", "'")).toContain(error);
  });

  it("offers Draft before any draft, and after a refused text", () => {
    expect(render(false, null)).toContain(">Draft</button>");
    expect(render(false, "This Source Text is too short.")).toContain(">Draft</button>");
  });
});
