import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { installDesktopBridge } from "./runtime/desktopBridge";
import "./styles/app.css";

installDesktopBridge();

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<App />
	</StrictMode>
);
