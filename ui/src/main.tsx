import { StrictMode, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./fonts.css";
import "./style.css";

// Dev build only: `?preview=play` shows the Play screen on fixture data,
// `?preview=journal` the Journal, `?preview=home` the home screen and `?preview=review` the draft review. A
// production build drops these branches and the fixtures with them.
const preview = new URLSearchParams(window.location.search).get("preview");
const Root: ComponentType =
  import.meta.env.DEV && preview === "play"
    ? (await import("./play/PlayPreview")).default
    : import.meta.env.DEV && preview === "journal"
      ? (await import("./journal/JournalPreview")).default
      : import.meta.env.DEV && preview === "home"
        ? (await import("./home/HomePreview")).default
        : import.meta.env.DEV && preview === "review"
          ? (await import("./views/ReviewPreview")).default
          : App;

const root = document.getElementById("root");
if (!root) throw new Error("no #root element");
createRoot(root).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
