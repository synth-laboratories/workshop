import React from "react";
import { createRoot } from "react-dom/client";
import { EnvironmentQaPage } from "../../../../apps/synth_desktop/src/renderer/src/components/EnvironmentQaPage";

createRoot(document.getElementById("root")!).render(
  <div style={{ height: "100vh", display: "flex", fontFamily: "system-ui" }}>
    <EnvironmentQaPage onBack={() => {}} serviceOrigin="http://127.0.0.1:17340" />
  </div>,
);
