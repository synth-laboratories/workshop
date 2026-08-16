import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bridges, installDesktopBridge } from "./runtime/desktopBridge";
import { installVisualDiagnosticSink } from "./runtime/diagnostics";
import "./styles/tokens.css";
import "./styles/primitives.css";
import "./styles/app.css";
import "./styles/usage.css";

installDesktopBridge();
// Visual bundles emit through a host-installed sink; without it they are silent.
installVisualDiagnosticSink();
void bridges.desktop.getInstanceDiagnostics().then((identity) => {
	document.title = identity.displayName;
	document.documentElement.dataset.desktopInstance = identity.name ?? "canonical";
}).catch(() => undefined);

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<App />
	</StrictMode>
);
