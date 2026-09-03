/**
 * Layout findings measured from the real rendered DOM at capture time.
 *
 * A screenshot proves a state rendered; it does not say whether the state is
 * defensible. These rules are the subset of the visual standard a machine can
 * actually decide — geometry, clipping, legibility, and how much of a surface
 * is placeholder — so a review can flag them without a human, and a human's
 * attention goes to the judgements that genuinely need it.
 *
 * The rules are pure functions over measurements. The DOM only enters through
 * `collectSurface`, which is deliberately the only untestable part.
 */

export type SurfaceRect = { x: number; y: number; width: number; height: number };

export type SurfaceElement = {
	tag: string;
	testid?: string;
	className?: string;
	rect: SurfaceRect;
	scrollWidth: number;
	clientWidth: number;
	fontSize: number;
	/** Trimmed own-text, capped. Empty for pure layout containers. */
	text: string;
	/** Computed `overflow-x` and `text-overflow`, to tell clipping from wrapping. */
	overflowX: string;
	textOverflow: string;
};

export type FindingCategory =
	| "responsive-geometry"
	| "truncation"
	| "hierarchy-density"
	| "missing-evidence";

export type Finding = {
	category: FindingCategory;
	rule: string;
	/** "egregious" is mechanically decidable and safe to fix without a human. */
	severity: "egregious" | "review";
	target: string;
	detail: string;
};

/** Placeholder tokens the workspaces use for an unmeasured value. */
const EMPTY_TOKENS = new Set(["—", "-", "–", "n/a", "not measured", "unavailable", "pending", "none yet"]);

export function describe(element: SurfaceElement): string {
	if (element.testid) return `[data-testid="${element.testid}"]`;
	const cls = (element.className ?? "").trim().split(/\s+/).filter(Boolean)[0];
	return cls ? `${element.tag}.${cls}` : element.tag;
}

/**
 * A box crossing the viewport's right edge. This is the defect that produces a
 * horizontally scrolling pane, and it is never intentional in this app.
 */
export function findHorizontalOverflow(
	elements: SurfaceElement[],
	viewport: { width: number }
): Finding[] {
	const limit = viewport.width + 1;
	return elements
		.filter((element) => element.rect.width > 0 && element.rect.x + element.rect.width > limit + 4)
		.map((element) => ({
			category: "responsive-geometry" as const,
			rule: "horizontal-overflow",
			severity: "egregious" as const,
			target: describe(element),
			detail: `right edge ${Math.round(element.rect.x + element.rect.width)}px exceeds the ${viewport.width}px viewport`
		}));
}

/**
 * Text actually clipped by its box. `scrollWidth > clientWidth` alone is not
 * enough: a wrapping element legitimately scrolls, so only elements whose
 * computed style says they hide the overflow are reported.
 */
export function findClippedText(elements: SurfaceElement[]): Finding[] {
	return elements
		.filter((element) => {
			if (!element.text || element.clientWidth <= 0) return false;
			const clips = element.textOverflow === "ellipsis" || element.overflowX === "hidden";
			return clips && element.scrollWidth > element.clientWidth + 1;
		})
		.map((element) => ({
			category: "truncation" as const,
			rule: "clipped-text",
			// Ellipsis is often a deliberate choice; a human decides whether the
			// clipped word was the one that mattered.
			severity: "review" as const,
			target: describe(element),
			detail: `"${element.text.slice(0, 48)}" needs ${element.scrollWidth}px in a ${element.clientWidth}px box`
		}));
}

/** Text too small to read at 100% zoom. */
export function findIllegibleText(elements: SurfaceElement[], floor = 9): Finding[] {
	return elements
		.filter((element) => element.text.length > 0 && element.fontSize > 0 && element.fontSize < floor)
		.map((element) => ({
			category: "hierarchy-density" as const,
			rule: "illegible-text",
			severity: "egregious" as const,
			target: describe(element),
			detail: `${element.fontSize}px is below the ${floor}px floor`
		}));
}

/**
 * A surface that is mostly placeholder. One "—" is honest missingness; a panel
 * where most leaf values are placeholders is a surface with nothing to say, and
 * it reads to a user as broken rather than as empty.
 */
export function findPlaceholderSaturation(elements: SurfaceElement[], threshold = 0.6): Finding[] {
	const leaves = elements.filter((element) => element.text.length > 0 && element.text.length <= 24);
	if (leaves.length < 6) return [];
	const empty = leaves.filter((element) => EMPTY_TOKENS.has(element.text.trim().toLowerCase()));
	const ratio = empty.length / leaves.length;
	if (ratio < threshold) return [];
	return [{
		category: "missing-evidence",
		rule: "placeholder-saturation",
		severity: "review",
		target: "surface",
		detail: `${empty.length} of ${leaves.length} short values are placeholders (${Math.round(ratio * 100)}%)`
	}];
}

export type SurfaceAudit = {
	viewport: { width: number; height: number };
	elementCount: number;
	findings: Finding[];
	counts: Record<string, number>;
};

export function auditElements(
	elements: SurfaceElement[],
	viewport: { width: number; height: number }
): SurfaceAudit {
	const findings = [
		...findHorizontalOverflow(elements, viewport),
		...findClippedText(elements),
		...findIllegibleText(elements),
		...findPlaceholderSaturation(elements)
	];
	const counts: Record<string, number> = {};
	for (const finding of findings) {
		counts[finding.rule] = (counts[finding.rule] ?? 0) + 1;
	}
	return { viewport, elementCount: elements.length, findings, counts };
}

export function rectIntersectsViewport(
	rect: SurfaceRect,
	viewport: { width: number; height: number }
): boolean {
	return rect.x + rect.width > 0
		&& rect.y + rect.height > 0
		&& rect.x < viewport.width
		&& rect.y < viewport.height;
}

/** Own text only: a container's text is its children's, and reporting it again
 * would multiply every leaf defect by its depth. Capped so one prose block
 * cannot dominate the record. */
function ownText(element: Element): string {
	let text = "";
	for (const node of element.childNodes) {
		if (node.nodeType === Node.TEXT_NODE) text += node.textContent ?? "";
	}
	return text.trim().slice(0, 120);
}

/**
 * The only DOM-dependent step. Bounded because a dense workspace can carry
 * thousands of nodes and the host is holding a resized window while this runs.
 */
export function collectSurface(root: Document, limit = 4000): SurfaceElement[] {
	const out: SurfaceElement[] = [];
	for (const element of root.querySelectorAll("body *")) {
		if (out.length >= limit) break;
		const rect = element.getBoundingClientRect();
		// Nothing is decidable about a box with no area.
		if (rect.width <= 0 || rect.height <= 0) continue;
		// The PNG is a viewport, not the entire scrollable DOM. Off-screen rows
		// and descendants of a closed disclosure are not visible evidence and
		// must not generate findings for pixels that were never photographed.
		if (!rectIntersectsViewport(rect, { width: window.innerWidth, height: window.innerHeight })) continue;
		const closedDetails = element.closest("details:not([open])");
		if (closedDetails && element !== closedDetails) {
			const summary = closedDetails.querySelector(":scope > summary");
			if (element !== summary && !summary?.contains(element)) continue;
		}
		const style = window.getComputedStyle(element);
		if (style.visibility === "hidden" || style.display === "none") continue;
		out.push({
			tag: element.tagName.toLowerCase(),
			testid: (element as HTMLElement).dataset?.testid,
			className: typeof element.className === "string" ? element.className : undefined,
			rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
			scrollWidth: element.scrollWidth,
			clientWidth: element.clientWidth,
			fontSize: Number.parseFloat(style.fontSize) || 0,
			text: ownText(element),
			overflowX: style.overflowX,
			textOverflow: style.textOverflow
		});
	}
	return out;
}

/** Installed on `window` so the host can harvest an audit in one `eval`. */
export function installCaptureAudit(): void {
	(window as Window & { __synthCaptureAudit?: () => string }).__synthCaptureAudit = () =>
		JSON.stringify(
			auditElements(collectSurface(document), {
				width: window.innerWidth,
				height: window.innerHeight
			})
		);
}
