import { StrictMode, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./fonts.css";
import "./style.css";

// Dev build only: `?preview=play` shows the Play screen on fixture data.
// A production build drops this branch and the fixture with it.
const preview =
  import.meta.env.DEV && new URLSearchParams(window.location.search).get("preview") === "play";
const Root: ComponentType = preview ? (await import("./play/PlayPreview")).default : App;

const root = document.getElementById("root");
if (!root) throw new Error("no #root element");
createRoot(root).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
