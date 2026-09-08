import { migrateLegacyPreferences } from "./schema";
import { invoke } from "@tauri-apps/api/core";

type Entry = { value: string | null; revision: number };
type Snapshot = { entries: Record<string, Entry> };
const native = typeof window !== "undefined" && (window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window);
const entries = new Map<string, Entry>();
let queue: Promise<void> = Promise.resolve();
let generation = 0;
let initialized = false;
let changes = 0;
const supported = (key: string) => ["synth.preferences.v1", "synth.archivedContainerIds", "synth.inferenceRailDefaultV2", "synth.inferenceRailOpen", "synth.workbenchSidePanelWidth", "synth.workbenchZoomPercent", "synth.accountChoiceMade", "synth.training.lastRunId", "synth.training.lastPlacement"].includes(key) || /^synth\.models\.[a-zA-Z0-9._-]+$/.test(key);

function publish() { window.dispatchEvent(new Event("workshop:state-changed")); }
async function refresh() {
	const atStart = changes;
	const snapshot = await invoke<Snapshot>("desktop_state_get");
	if (atStart !== changes) return;
	let changed = false;
	for (const [key, entry] of Object.entries(snapshot.entries)) {
		if (entries.get(key)?.revision === entry.revision) continue;
		entries.set(key, entry);
		changed = true;
	}
	if (changed) publish();
}

async function recoverWrite(error: unknown) {
	generation++;
	entries.clear();
	try { await refresh(); }
	catch (refreshError) { console.error("Desktop state recovery awaits runtime reconnect", refreshError); }
	// A failed refresh must not reject the write queue permanently. Polling and
	// subsequent writes can recover after the runtime reconnects.
	window.dispatchEvent(new CustomEvent("workshop:state-error", { detail: `Desktop preferences were not saved: ${String(error)}` }));
}

/** Runtime-backed Storage port. Local storage is read only for one-time
 * migration; standalone browser previews retain their browser-only behavior. */
export const runtimeStorage: Storage = {
	get length() { return window.localStorage.length; },
	key(index) { return window.localStorage.key(index); },
	getItem(key) {
		if (native && initialized && supported(key)) return entries.get(key)?.value ?? null;
		return window.localStorage.getItem(key);
	},
	setItem(key, value) {
		if (!native || !initialized || !supported(key)) { window.localStorage.setItem(key, value); return; }
		const current = entries.get(key) ?? { value: null, revision: 0 };
		if (current.value === value) return;
		changes++;
		const epoch = generation;
		entries.set(key, { value, revision: current.revision + 1 });
		queue = queue.then(async () => {
			if (generation !== epoch) return;
			try {
				await invoke<Entry>("desktop_state_commit", { input: { key, value, expectedRevision: current.revision } });
			} catch (error) {
				await recoverWrite(error);
			}
		});
	},
	removeItem(key) {
		if (!native || !initialized || !supported(key)) { window.localStorage.removeItem(key); return; }
		const current = entries.get(key) ?? { value: null, revision: 0 };
		changes++;
		const epoch = generation;
		entries.set(key, { value: null, revision: current.revision + 1 });
		queue = queue.then(async () => {
			if (generation !== epoch) return;
			try { await invoke("desktop_state_commit", { input: { key, value: null, expectedRevision: current.revision } }); }
			catch (error) { await recoverWrite(error); }
		});
	},
	clear() { throw new Error("Clear individual desktop state keys explicitly"); }
};

export async function initializeRuntimeStorage(): Promise<void> {
	if (!native) return;
	await refresh();
	if ((entries.get("synth.preferences.v1")?.revision ?? 0) === 0) {
		const value = JSON.stringify(migrateLegacyPreferences(window.localStorage));
		try { entries.set("synth.preferences.v1", await invoke<Entry>("desktop_state_commit", { input: { key: "synth.preferences.v1", value, expectedRevision: 0 } })); }
		catch { await refresh(); }
	}
	// A persisted null is a tombstone, not permission to resurrect a legacy key.
	for (let index = 0; index < window.localStorage.length; index++) {
		const key = window.localStorage.key(index);
		if (!key || !supported(key) || (entries.get(key)?.revision ?? 0) > 0) continue;
		const value = window.localStorage.getItem(key);
		try { entries.set(key, await invoke<Entry>("desktop_state_commit", { input: { key, value, expectedRevision: 0 } })); }
		catch { await refresh(); }
	}
	initialized = true;
	publish();
	window.setInterval(() => { void queue.then(refresh).catch((error) => console.error("Desktop state refresh failed", error)); }, 1500);
}
