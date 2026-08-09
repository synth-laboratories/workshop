import { app, BrowserWindow, nativeImage, shell } from "electron";
import { existsSync } from "node:fs";
import { join } from "node:path";

const isDev = !app.isPackaged;
const APP_NAME = "Synth Dev";

/** Prefer PNG for dock — icns often fails to paint in electron-vite dev. */
function resolveAppIcon(): string | undefined {
	const roots = [join(__dirname, "../../resources"), join(app.getAppPath(), "resources")];
	for (const root of roots) {
		for (const name of ["icon.png", "icon.icns"]) {
			const candidate = join(root, name);
			if (existsSync(candidate)) return candidate;
		}
	}
	return undefined;
}

function applyDockIdentity(): void {
	app.setName(APP_NAME);
	if (process.platform !== "darwin" || !app.dock) return;
	const iconPath = resolveAppIcon();
	if (!iconPath) return;
	const img = nativeImage.createFromPath(iconPath);
	if (!img.isEmpty()) {
		app.dock.setIcon(img);
	}
}

function createWindow(): void {
	const iconPath = resolveAppIcon();
	const icon = iconPath ? nativeImage.createFromPath(iconPath) : undefined;

	const mainWindow = new BrowserWindow({
		width: 1280,
		height: 840,
		minWidth: 960,
		minHeight: 640,
		show: false,
		title: "Synth MOCK",
		backgroundColor: "#f3f5f8",
		...(icon && !icon.isEmpty() ? { icon } : {}),
		titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
		trafficLightPosition: { x: 16, y: 13 },
		movable: true,
		webPreferences: {
			preload: join(__dirname, "../preload/index.js"),
			sandbox: false,
			contextIsolation: true
		}
	});

	mainWindow.on("ready-to-show", () => {
		applyDockIdentity();
		mainWindow.setTitle("Synth MOCK");
		mainWindow.show();
	});

	mainWindow.webContents.setWindowOpenHandler((details) => {
		void shell.openExternal(details.url);
		return { action: "deny" };
	});

	if (isDev && process.env.ELECTRON_RENDERER_URL) {
		mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
	} else {
		mainWindow.loadFile(join(__dirname, "../renderer/index.html"));
	}
}

// Name must be set before ready for Dock tooltip.
app.setName(APP_NAME);

app.whenReady().then(() => {
	applyDockIdentity();
	createWindow();

	app.on("activate", () => {
		if (BrowserWindow.getAllWindows().length === 0) createWindow();
	});
});

app.on("window-all-closed", () => {
	if (process.platform !== "darwin") app.quit();
});
