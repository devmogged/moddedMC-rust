import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyAppearance } from "./settings";
import "./theme.css";

applyAppearance();
document.addEventListener("contextmenu", (event) => event.preventDefault());
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
