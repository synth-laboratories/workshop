import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bridges, installDesktopBridge } from "./runtime/desktopBridge";
import "./styles/app.css";

installDesktopBridge();
void bridges.desktop.getInstanceDiagnostics().then((identity) => {
	document.title = identity.displayName;
	document.documentElement.dataset.desktopInstance = identity.name ?? "canonical";
}).catch(() => undefined);

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<App />
	</StrictMode>
);
