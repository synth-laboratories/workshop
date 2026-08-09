import { contextBridge, ipcRenderer } from "electron";

type RequestOptions = {
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

type SubscriptionCallbacks = {
	onEvent: (event: unknown) => void;
	onStatus?: (status: { state: string; detail?: string }) => void;
};

type SubscriptionMessage = {
	subscriptionId: string;
	type: "event" | "status";
	event?: unknown;
	status?: { state: string; detail?: string };
};

type LagunaStatus = {
	phase: "unknown" | "starting" | "loading" | "ready" | "error" | "unavailable";
	baseUrl: string | null;
	backend: string | null;
	loadedModel: string | null;
	detail: string | null;
	memoryBytes: number | null;
	updatedAt: number;
};

const callbacks = new Map<string, SubscriptionCallbacks>();

ipcRenderer.on("runtime:subscription", (_event, message: SubscriptionMessage) => {
	const callback = callbacks.get(message.subscriptionId);
	if (!callback) return;
	if (message.type === "event" && message.event !== undefined) {
		callback.onEvent(message.event);
	}
	if (message.type === "status" && message.status) {
		callback.onStatus?.(message.status);
	}
});

contextBridge.exposeInMainWorld("synthDesktop", {
	platform: process.platform,
	chooseProjectDirectory(): Promise<string | null> {
		return ipcRenderer.invoke("project:chooseDirectory");
	}
});

contextBridge.exposeInMainWorld("synthLaguna", {
	getStatus(): Promise<LagunaStatus> {
		return ipcRenderer.invoke("laguna:getStatus");
	},
	onStatus(listener: (status: LagunaStatus) => void): () => void {
		const handler = (_event: Electron.IpcRendererEvent, status: LagunaStatus) => {
			listener(status);
		};
		ipcRenderer.on("laguna:status", handler);
		return () => {
			ipcRenderer.removeListener("laguna:status", handler);
		};
	}
});

contextBridge.exposeInMainWorld("synthRuntime", {
	request(path: string, options: RequestOptions = {}) {
		return ipcRenderer.invoke("runtime:request", {
			path,
			method: options.method || "GET",
			body: options.body
		});
	},

	async subscribe(
		sessionId: string,
		afterSequence: number,
		onEvent: (event: unknown) => void,
		onStatus?: (status: { state: string; detail?: string }) => void
	) {
		const subscriptionId = crypto.randomUUID();
		callbacks.set(subscriptionId, { onEvent, onStatus });
		try {
			await ipcRenderer.invoke("runtime:subscribe", {
				subscriptionId,
				sessionId,
				afterSequence
			});
		} catch (error) {
			callbacks.delete(subscriptionId);
			throw error;
		}
		return {
			close() {
				callbacks.delete(subscriptionId);
				ipcRenderer.send("runtime:unsubscribe", subscriptionId);
			}
		};
	}
});
