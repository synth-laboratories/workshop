export function parseLiveGepaJsonl(text: string): Array<Record<string, any>> {
	const lines = text.split(/\r?\n/);
	const events: Array<Record<string, any>> = [];
	for (const [index, line] of lines.entries()) {
		if (!line.trim()) continue;
		try {
			events.push(JSON.parse(line));
		} catch (error) {
			// A producer may be midway through appending the last JSONL record.
			// Malformed complete records remain hard failures: only an unterminated
			// final fragment is safe to defer until the next poll.
			const isTrailingPartial = index === lines.length - 1 && !text.endsWith("\n");
			if (isTrailingPartial) break;
			throw new Error(`invalid GEPA JSONL at line ${index + 1}: ${error instanceof Error ? error.message : String(error)}`);
		}
	}
	return events;
}
