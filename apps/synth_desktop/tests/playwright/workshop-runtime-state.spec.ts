import { expect, test } from "./browser.fixture";

test("desktop storage migrates once, follows runtime updates, and surfaces stale writes", async ({ page }) => {
	const origin = new URL(page.url()).origin;
	await page.route("**/state-harness", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><html><body>Runtime storage harness</body></html>" }));
	await page.addInitScript(() => {
		const w = window as any;
		w.__entries = {};
		w.__stateErrors = [];
		w.__race = false;
		window.addEventListener("workshop:state-error", (event) => w.__stateErrors.push((event as CustomEvent).detail));
		w.__TAURI_INTERNALS__ = { invoke: async (name: string, args: any) => {
			if (w.__offline) throw new Error("runtime offline");
			if (name === "desktop_state_get") return { entries: structuredClone(w.__entries) };
			if (name !== "desktop_state_commit") throw new Error(`Unexpected command ${name}`);
			const { key, value, expectedRevision } = args.input;
			if (w.__race) { w.__race = false; w.__entries[key] = { value: "0", revision: expectedRevision + 1 }; }
			if ((w.__entries[key]?.revision ?? 0) !== expectedRevision) throw new Error("desktop_state_conflict");
			return w.__entries[key] = { value, revision: expectedRevision + 1 };
		} };
	});
	await page.goto(`${origin}/state-harness`);
	await page.evaluate(async () => {
		const w = window as any;
		localStorage.setItem("synth.inferenceRailOpen", "1");
		const modulePath = "/src/preferences/runtimeStorage.ts";
		w.__storage = await import(/* @vite-ignore */ modulePath);
		await w.__storage.initializeRuntimeStorage();
	});
	await expect.poll(() => page.evaluate(() => (window as any).__entries["synth.inferenceRailOpen"])).toEqual({ value: "1", revision: 1 });
	await page.evaluate(() => {
		const w = window as any;
		w.__entries["synth.inferenceRailOpen"] = { value: "0", revision: 2 };
	});
	await expect.poll(() => page.evaluate(() => (window as any).__storage.runtimeStorage.getItem("synth.inferenceRailOpen"))).toBe("0");
	await page.evaluate(() => {
		const w = window as any;
		w.__race = true;
		w.__storage.runtimeStorage.setItem("synth.inferenceRailOpen", "1");
	});
	await expect.poll(() => page.evaluate(() => (window as any).__stateErrors.length)).toBe(1);
	await expect.poll(() => page.evaluate(() => (window as any).__storage.runtimeStorage.getItem("synth.inferenceRailOpen"))).toBe("0");
	await expect.poll(() => page.evaluate(() => (window as any).__entries["synth.inferenceRailOpen"])).toEqual({ value: "0", revision: 3 });
	await page.evaluate(() => {
		const w = window as any;
		w.__offline = true;
		w.__storage.runtimeStorage.setItem("synth.inferenceRailOpen", "1");
	});
	await expect.poll(() => page.evaluate(() => (window as any).__stateErrors.length)).toBe(2);
	await page.evaluate(() => { (window as any).__offline = false; });
	await expect.poll(() => page.evaluate(() => (window as any).__storage.runtimeStorage.getItem("synth.inferenceRailOpen"))).toBe("0");
	await page.evaluate(() => (window as any).__storage.runtimeStorage.setItem("synth.inferenceRailOpen", "1"));
	await expect.poll(() => page.evaluate(() => (window as any).__entries["synth.inferenceRailOpen"])).toEqual({ value: "1", revision: 4 });

});
