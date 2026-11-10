import { StrictMode, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./fonts.css";
import "./style.css";

// Dev build only: `?preview=play` shows the Play screen on fixture data, and
// `?preview=journal` the Journal. A production build drops these branches and
// the fixtures with them.
const preview = new URLSearchParams(window.location.search).get("preview");
const Root: ComponentType =
  import.meta.env.DEV && preview === "play"
    ? (await import("./play/PlayPreview")).default
    : import.meta.env.DEV && preview === "journal"
      ? (await import("./journal/JournalPreview")).default
      : App;

const root = document.getElementById("root");
if (!root) throw new Error("no #root element");
createRoot(root).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
