/** Decimal USD strings for conversation paid-compute limits. Integer micros live on the host. */

export type UsdParseResult = { micros: number; error: null } | { micros: null; error: string };

export function parseUsdAmount(value: string): UsdParseResult {
	const trimmed = value.trim();
	if (!trimmed) return { micros: null, error: "Enter a USD amount." };
	if (trimmed.startsWith("-") || trimmed.startsWith("+")) {
		return { micros: null, error: "Amount must not be negative or signed." };
	}
	if (/[eE]/.test(trimmed)) {
		return { micros: null, error: "Amount must not use exponent notation." };
	}
	const [whole, frac = "", extra] = trimmed.split(".");
	if (extra !== undefined || !whole || !/^\d+$/.test(whole) || (frac && !/^\d+$/.test(frac))) {
		return { micros: null, error: "Amount must be a decimal USD string." };
	}
	if (frac.length > 6) {
		return { micros: null, error: "Amount may have at most six fractional digits." };
	}
	const wholeMicros = Number(whole) * 1_000_000;
	const fracMicros = frac ? Number(frac.padEnd(6, "0")) : 0;
	if (!Number.isSafeInteger(wholeMicros + fracMicros)) {
		return { micros: null, error: "Amount is out of range." };
	}
	return { micros: wholeMicros + fracMicros, error: null };
}

export function formatUsdMicros(micros: number): string {
	const dollars = Math.trunc(micros / 1_000_000);
	const rem = Math.abs(micros % 1_000_000);
	if (rem === 0) return `${dollars}.00`;
	if (rem % 10_000 === 0) return `${dollars}.${String(rem / 10_000).padStart(2, "0")}`;
	return `${dollars}.${String(rem).padStart(6, "0").replace(/0+$/, "")}`;
}
