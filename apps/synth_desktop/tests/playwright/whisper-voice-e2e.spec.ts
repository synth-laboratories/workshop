import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

type VoiceFixtureOptions = {
	installed?: boolean;
	transcriptions?: string[];
	warmError?: string;
	transcribeError?: string;
	microphoneAvailable?: boolean;
	initialPhase?: "unloaded" | "warming" | "ready" | "transcribing" | "error";
};

async function installVoiceFixture(page: Page, options: VoiceFixtureOptions = {}): Promise<void> {
	await page.addInitScript((fixture) => {
		const root = window as typeof window & {
			__voice: {
				calls: Array<{ name: string; args?: unknown[] }>;
				listeners: Array<(status: unknown) => void>;
				transcriptions: string[];
				emit(status: unknown): void;
			};
			synthLaguna?: unknown;
			synthWhisper?: unknown;
		};
		const ready = {
			phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
			loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "ready", memoryBytes: 1,
			updatedAt: Date.now()
		};
		root.synthLaguna = {
			getStatus: async () => ready, reload: async () => ready, onStatus: () => () => undefined,
			listModels: async () => [], chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("unused"); },
			clearModelDirectory: async () => undefined
		};

		const installed = fixture.installed !== false;
		const runtime = (phase: string) => ({
			phase, loadedModel: phase === "unloaded" ? null : "large-v3-turbo",
			idleSeconds: 0, idleUnloadAfterSeconds: 900, lastUsedAt: Date.now(),
			freeAt: Date.now() + 900_000, updatedAt: Date.now()
		});
		root.__voice = {
			calls: [], listeners: [], transcriptions: fixture.transcriptions ?? ["Hey can you hear this?"],
			emit(status) { for (const listener of this.listeners) listener(status); }
		};
		root.synthWhisper = {
			listModels: async () => [{
				id: "large-v3-turbo", title: "Whisper Large v3 Turbo", recommended: false,
				multilingual: true, downloadBytes: 1_549_000_000,
				installedBytes: installed ? 1_549_000_000 : null,
				path: installed ? "/models/whisper/large-v3-turbo" : null,
				selected: installed, modelsRoot: "/models/whisper"
			}],
			downloadModel: async () => { throw new Error("unused"); },
			setSelected: async () => undefined, clearModel: async () => undefined,
			getRuntimeStatus: async () => runtime(fixture.initialPhase ?? "unloaded"),
			onRuntimeStatus: (listener: (status: unknown) => void) => {
				root.__voice.listeners.push(listener);
				return () => { root.__voice.listeners = root.__voice.listeners.filter((item) => item !== listener); };
			},
			warmSelected: async () => {
				root.__voice.calls.push({ name: "warm" });
				root.__voice.emit(runtime("warming"));
				if (fixture.warmError) {
					root.__voice.emit(runtime("error"));
					throw new Error(fixture.warmError);
				}
				root.__voice.emit(runtime("ready"));
				return runtime("ready");
			},
			transcribe: async () => "",
			transcribeAudio: async (base64: string, mimeType: string) => {
				root.__voice.calls.push({ name: "transcribe", args: [base64, mimeType] });
				root.__voice.emit(runtime("transcribing"));
				if (fixture.transcribeError) throw new Error(fixture.transcribeError);
				const text = root.__voice.transcriptions.shift() ?? "";
				root.__voice.emit(runtime("ready"));
				return text;
			}
		};

		if (fixture.microphoneAvailable !== false) {
			Object.defineProperty(navigator, "mediaDevices", {
				configurable: true,
				value: { getUserMedia: async () => ({ getTracks: () => [{ stop() {} }] }) }
			});
		} else {
			Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: undefined });
		}
		class FakeMediaRecorder {
			static isTypeSupported() { return true; }
			mimeType: string;
			ondataavailable: ((event: { data: Blob }) => void) | null = null;
			onstop: (() => void) | null = null;
			constructor(_stream: unknown, settings?: { mimeType?: string }) { this.mimeType = settings?.mimeType ?? "audio/mp4"; }
			start() {}
			stop() {
				this.ondataavailable?.({ data: new Blob([new Uint8Array([1, 2, 3])], { type: this.mimeType }) });
				this.onstop?.();
			}
		}
		class FakeAudioContext {
			async decodeAudioData() {
				return { duration: 0.1, sampleRate: 16_000, length: 1_600, numberOfChannels: 1, getChannelData: () => new Float32Array(1_600).fill(0.1) };
			}
			async close() {}
		}
		Object.defineProperty(window, "MediaRecorder", { configurable: true, value: FakeMediaRecorder });
		Object.defineProperty(window, "AudioContext", { configurable: true, value: FakeAudioContext });
	}, options);
	await page.reload();
	await expect(page.getByTestId("composer-input")).toBeEnabled();
}

async function recordOnce(page: Page): Promise<void> {
	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("composer-mic")).toHaveAccessibleName("Stop recording");
	await page.getByTestId("composer-mic").click();
}

test("clicking the mic warms Whisper while recording begins", async ({ page }) => {
	await installVoiceFixture(page);
	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("composer-whisper-status")).toContainText(/Whisper ready|Warming Whisper/);
	expect(await page.evaluate(() => (window as any).__voice.calls.map((call: any) => call.name))).toEqual(["warm"]);
});

test("stopping a recording inserts the English transcription", async ({ page }) => {
	await installVoiceFixture(page);
	await recordOnce(page);
	await expect(page.getByTestId("composer-input")).toHaveValue("Hey can you hear this?");
});

test("voice text appends to an existing draft without destroying it", async ({ page }) => {
	await installVoiceFixture(page, { transcriptions: ["added by voice"] });
	await page.getByTestId("composer-input").fill("Keep this");
	await recordOnce(page);
	await expect(page.getByTestId("composer-input")).toHaveValue("Keep this added by voice");
});

test("two recordings reuse the lifecycle and both produce text", async ({ page }) => {
	await installVoiceFixture(page, { transcriptions: ["first phrase", "second phrase"] });
	await recordOnce(page);
	await recordOnce(page);
	await expect(page.getByTestId("composer-input")).toHaveValue("first phrase second phrase");
	expect(await page.evaluate(() => (window as any).__voice.calls.map((call: any) => call.name))).toEqual([
		"warm", "transcribe", "warm", "transcribe"
	]);
});

test("recorded audio crosses the bridge as a nonempty WAV payload", async ({ page }) => {
	await installVoiceFixture(page);
	await recordOnce(page);
	await expect.poll(() => page.evaluate(() => (window as any).__voice.calls.some((item: any) => item.name === "transcribe"))).toBe(true);
	const call = await page.evaluate(() => (window as any).__voice.calls.find((item: any) => item.name === "transcribe"));
	expect(call.args[1]).toBe("audio/wav");
	expect(call.args[0].length).toBeGreaterThan(100);
});

test("an empty transcription leaves the draft unchanged", async ({ page }) => {
	await installVoiceFixture(page, { transcriptions: [""] });
	await page.getByTestId("composer-input").fill("existing draft");
	await recordOnce(page);
	await expect(page.getByTestId("composer-input")).toHaveValue("existing draft");
});

test("transcription failures show a useful error and never insert garbage", async ({ page }) => {
	await installVoiceFixture(page, { transcribeError: "I couldn't understand that recording. Please try again." });
	await recordOnce(page);
	await expect(page.getByTestId("composer-mic-error")).toContainText("couldn't understand");
	await expect(page.getByTestId("composer-input")).toHaveValue("");
});

test("warmup failures are visible without preventing the user from stopping recording", async ({ page }) => {
	await installVoiceFixture(page, { warmError: "Whisper runtime failed to warm" });
	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("composer-mic-error")).toContainText("failed to warm");
	await expect(page.getByTestId("composer-mic")).toHaveAccessibleName("Stop recording");
});

test("runtime phases render warming, transcribing, ready, and unloaded states", async ({ page }) => {
	await installVoiceFixture(page);
	for (const [phase, label] of [["warming", "Warming Whisper"], ["transcribing", "Transcribing"], ["ready", "releases after 15 min idle"]] as const) {
		await page.evaluate((next) => (window as any).__voice.emit({ phase: next, loadedModel: "large-v3-turbo", idleUnloadAfterSeconds: 900, updatedAt: Date.now() }), phase);
		await expect(page.getByTestId("composer-whisper-status")).toContainText(label);
	}
	await page.evaluate(() => (window as any).__voice.emit({ phase: "unloaded", loadedModel: null, idleUnloadAfterSeconds: 900, updatedAt: Date.now() }));
	await expect(page.getByTestId("composer-whisper-status")).toHaveCount(0);
});

test("ready status communicates the same 15-minute idle policy as Laguna", async ({ page }) => {
	await installVoiceFixture(page, { initialPhase: "ready" });
	await expect(page.getByTestId("composer-whisper-status")).toHaveText(/Whisper ready · releases after 15 min idle/);
});

test("missing model routes directly to Voice settings and never requests the microphone", async ({ page }) => {
	await installVoiceFixture(page, { installed: false });
	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("voice-recognition-settings")).toBeVisible();
	expect(await page.evaluate(() => (window as any).__voice.calls)).toEqual([]);
});

test("missing mediaDevices produces the restart and permission guidance", async ({ page }) => {
	await installVoiceFixture(page, { microphoneAvailable: false });
	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("composer-mic-error")).toContainText(/Microphone capture is unavailable|Privacy & Security/);
});
