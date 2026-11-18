import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { BadgeApp } from "./features/overlays/BadgeApp";
import { HudApp } from "./features/overlays/HudApp";
import { RegionApp } from "./features/overlays/RegionApp";
import "./styles.css";

type View = "main" | "badge" | "region" | "hud";

/** The badge and the region overlay are separate windows on the same page, told apart by the hash. */
function currentView(): View {
  switch (window.location.hash) {
    case "#badge":
      return "badge";
    case "#hud":
      return "hud";
    case "#region":
      return "region";
    default:
      return "main";
  }
}

function render(view: View): ReactNode {
  switch (view) {
    case "main":
      return <App />;
    case "badge":
      return <BadgeApp />;
    case "hud":
      return <HudApp />;
    case "region":
      return <RegionApp />;
    default: {
      const unreachable: never = view;
      throw new Error(`Unknown view: ${String(unreachable)}`);
    }
  }
}

const view = currentView();
document.body.dataset.view = view;
// The app has no text fields that need the browser menu; keep it out of the way.
window.addEventListener("contextmenu", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) {
    event.preventDefault();
  }
});

const root = document.getElementById("root");
if (!root) {
  throw new Error("Root element is missing");
}
createRoot(root).render(<StrictMode>{render(view)}</StrictMode>);
