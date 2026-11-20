import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Shell } from "./Shell";
import { applyAppearance } from "./settings";
import "./theme.css";

applyAppearance();
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Shell />
    </StrictMode>,
  );
}
