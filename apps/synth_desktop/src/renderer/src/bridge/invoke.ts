/**
 * Specta-generated command bindings are the desktop invoke surface.
 * Call sites go through `commands` in `generated/protocol.ts`.
 */

export { commands as generatedCommands } from "../generated/protocol";

type SpectaEnvelope<D> = { status: "ok"; data: D } | { status: "error"; error: unknown };

type Unwrapped<T> = [T] extends [SpectaEnvelope<infer D>]
	? [D] extends [null]
		? void
		: D
	: [T] extends [SpectaEnvelope<infer D> | null]
		? [D] extends [null]
			? void
			: D
		: T;

function isSpectaEnvelope(value: unknown): value is SpectaEnvelope<unknown> {
	return (
		!!value &&
		typeof value === "object" &&
		"status" in value &&
		((value as SpectaEnvelope<unknown>).status === "ok" ||
			(value as SpectaEnvelope<unknown>).status === "error")
	);
}

/** Unwrap tauri-specta `typedError` envelopes; pass through raw invoke results. */
export async function fromGenerated<T>(result: Promise<T>): Promise<Unwrapped<T>> {
	const value = await result;
	if (isSpectaEnvelope(value)) {
		if (value.status === "error") throw value.error;
		return value.data as Unwrapped<T>;
	}
	return value as Unwrapped<T>;
}

/** Specta command args are `T | null`; renderer optionals are `T | undefined`. */
export function n<T>(value: T | null | undefined): T | null {
	return value ?? null;
}

/** Coerce a renderer-shaped value onto a generated command argument. */
export function wire<T>(value: unknown): T {
	return value as T;
}
