import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import type { Backend } from "./lib/tauri";

declare global {
  interface Window {
    __MDVIEWER_TEST_BACKEND__?: Backend;
  }
}

const root = document.getElementById("root");

if (!root) {
  throw new Error("MDViewer root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <App backend={window.__MDVIEWER_TEST_BACKEND__} />
  </StrictMode>,
);
