type JsonRecord = Record<string, unknown>;

function record(value: unknown): JsonRecord {
	return value && typeof value === "object" && !Array.isArray(value) ? value as JsonRecord : {};
}

function finiteSeed(...values: unknown[]): number | null {
	for (const value of values) {
		const parsed = typeof value === "string" ? Number(value) : value;
		if (typeof parsed === "number" && Number.isFinite(parsed)) return parsed;
	}
	return null;
}

function trialIdOf(event: JsonRecord): string {
	const delta = record(event.delta);
	const raw = record(event.raw);
	const item = record(event.item);
	const value = delta.trial_id ?? delta.trialId ?? raw.trial_id ?? raw.trialId ?? item.id;
	return typeof value === "string" ? value : "";
}

function containerEventOf(event: JsonRecord): JsonRecord {
	const delta = record(event.delta);
	const raw = record(event.raw);
	const rawEvent = record(raw.container_event ?? raw.containerEvent);
	const publicEvent = record(delta.containerEvent ?? delta.container_event);
	return {
		...rawEvent,
		...publicEvent,
		...((rawEvent.frame != null || publicEvent.frame != null)
			? { frame: { ...record(rawEvent.frame), ...record(publicEvent.frame) } }
			: {}),
	};
}

function withoutFrameBody(container: unknown): unknown {
	const value = record(container);
	const frame = record(value.frame);
	if (Object.keys(frame).length === 0 || (frame.data_url == null && frame.dataUrl == null)) return container;
	const { data_url: _snake, dataUrl: _camel, ...frameMetadata } = frame;
	return { ...value, frame: frameMetadata };
}

function compactEvent(event: unknown): unknown {
	const value = record(event);
	if (Object.keys(value).length === 0) return event;
	const delta = record(value.delta);
	const raw = record(value.raw);
	let changed = false;
	let nextDelta = delta;
	let nextRaw = raw;
	for (const key of ["containerEvent", "container_event"] as const) {
		if (delta[key] != null) {
			const compact = withoutFrameBody(delta[key]);
			if (compact !== delta[key]) { nextDelta = { ...nextDelta, [key]: compact }; changed = true; }
		}
		if (raw[key] != null) {
			const compact = withoutFrameBody(raw[key]);
			if (compact !== raw[key]) { nextRaw = { ...nextRaw, [key]: compact }; changed = true; }
		}
	}
	return changed ? { ...value, delta: nextDelta, raw: nextRaw } : event;
}

/** Fold the durable frame history into one bounded native observation per seed. */
export function projectManagedOptimizerPayload(value: unknown): unknown {
	const payload = record(value);
	if (!Array.isArray(payload.events)) return value;
	const enrichment = Array.isArray(payload.enrichmentEvents) ? payload.enrichmentEvents : [];
	const allEvents = [...payload.events, ...enrichment] as JsonRecord[];
	const trialSeeds = new Map<string, number>();
	const mediaBySeed: Record<string, JsonRecord> = {};

	for (const event of allEvents) {
		const delta = record(event.delta);
		const raw = record(event.raw);
		const item = record(event.item);
		const itemKey = record(record(item.raw).key);
		const trialId = trialIdOf(event);
		const declaredSeed = finiteSeed(delta.seed, raw.seed, item.seed, itemKey.seed);
		if (trialId && declaredSeed != null) trialSeeds.set(trialId, declaredSeed);
		if (event.type !== "eval.trial.event") continue;
		const container = containerEventOf(event);
		const frame = record(container.frame);
		const dataUrl = frame.data_url ?? frame.dataUrl;
		if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:image/png;base64,")) continue;
		const seed = finiteSeed(container.seed, declaredSeed, trialSeeds.get(trialId));
		if (seed == null) continue;
		mediaBySeed[String(seed)] = {
			frame_data_url: dataUrl,
			sequence_number: event.sequenceNumber ?? event.sequence_number ?? null,
			content_type: frame.content_type ?? frame.contentType ?? "image/png",
			sha256: frame.sha256 ?? null,
			width: frame.width ?? null,
			height: frame.height ?? null,
		};
	}

	return {
		...payload,
		events: payload.events.map(compactEvent),
		...(Array.isArray(payload.enrichmentEvents)
			? { enrichmentEvents: payload.enrichmentEvents.map(compactEvent) }
			: {}),
		mediaBySeed,
	};
}
